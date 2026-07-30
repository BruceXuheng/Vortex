package com.vortex.service

import android.os.ParcelFileDescriptor
import android.util.Log
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InterruptedIOException
import java.io.OutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future

/**
 * 双向转发 IP 包：VPN fd ↔ RelayConnection。
 *
 * 设备→网络方向：从 VPN fd 读取完整 IP 包，通过 RelayConnection 发送。
 * 网络→设备方向：从 RelayConnection 接收字节流，经 [IPPacketOutputStream] 切包后写入 VPN fd。
 */
class PacketForwarder(
    /** VPN 接口的文件描述符，用于读写 IP 包。 */
    private val vpnFd: ParcelFileDescriptor,
    /** 与 Relay Server 的连接，用于发送/接收 IP 包。 */
    private val relayConnection: RelayConnection,
    /** 转发异常回调，用于通知上层（VpnService）错误状态。 */
    private val onError: (String) -> Unit
) {

    companion object {
        private const val TAG = "VortexForwarder"
        /** 缓冲区大小（64KB）。 */
        private const val BUFFER_SIZE = 0x10000
        /** wakeUpReadWorkaround 使用的虚拟目标地址。 */
        private val DUMMY_ADDRESS = byteArrayOf(42, 42, 42, 42)
        private const val DUMMY_PORT = 4242
    }

    /** 双线程池，分别负责设备→网络和网络→设备方向的转发。 */
    private val executor: ExecutorService = Executors.newFixedThreadPool(2)
    /** 设备→网络转发线程的 Future，用于取消。 */
    private var deviceToNetworkFuture: Future<*>? = null
    /** 网络→设备转发线程的 Future，用于取消。 */
    private var networkToDeviceFuture: Future<*>? = null

    /** 启动双线程转发。 */
    fun start() {
        deviceToNetworkFuture = executor.submit { forwardDeviceToNetwork() }
        networkToDeviceFuture = executor.submit { forwardNetworkToDevice() }
    }

    /** 停止转发：关闭连接、取消线程、唤醒 VPN 读阻塞。 */
    fun stop() {
        relayConnection.close()
        networkToDeviceFuture?.cancel(true)
        deviceToNetworkFuture?.cancel(true)
        wakeUpReadWorkaround()
    }

    /**
     * 设备→网络：从 VPN fd 读取 IP 包，发给 Relay Server。
     *
     * VPN fd 的 FileInputStream.read() 每次返回一个完整 IP 包。
     */
    private fun forwardDeviceToNetwork() {
        try {
            Log.d(TAG, "设备→网络 转发线程启动")
            val input = FileInputStream(vpnFd.fileDescriptor)
            val buffer = ByteArray(BUFFER_SIZE)
            while (!Thread.currentThread().isInterrupted) {
                val bytesRead = input.read(buffer)
                if (bytesRead == -1) {
                    Log.d(TAG, "VPN fd 已关闭")
                    break
                }
                if (bytesRead > 0) {
                    val version = buffer[0].toInt() shr 4 and 0x0F
                    if (version == 4) {
                        relayConnection.send(buffer, bytesRead)
                    } else {
                        Log.w(TAG, "非 IPv4 包，版本=$version，已丢弃")
                    }
                }
            }
        } catch (e: InterruptedIOException) {
            Log.d(TAG, "设备→网络 线程被中断")
        } catch (e: IOException) {
            Log.e(TAG, "设备→网络 IO 异常", e)
            onError(e.message ?: "设备→网络 IO 异常")
        }
        Log.d(TAG, "设备→网络 转发线程结束")
    }

    /**
     * 网络→设备：从 Relay Server 接收数据，经 IPPacketOutputStream 写入 VPN fd。
     */
    private fun forwardNetworkToDevice() {
        try {
            Log.d(TAG, "网络→设备 转发线程启动")
            val output = FileOutputStream(vpnFd.fileDescriptor)
            val packetOutput = IPPacketOutputStream(output)
            val buffer = ByteArray(BUFFER_SIZE)
            while (!Thread.currentThread().isInterrupted) {
                val bytesRead = relayConnection.receive(buffer)
                if (bytesRead == -1) {
                    Log.d(TAG, "Relay 连接已关闭")
                    break
                }
                if (bytesRead > 0) {
                    packetOutput.write(buffer, 0, bytesRead)
                }
            }
        } catch (e: InterruptedIOException) {
            Log.d(TAG, "网络→设备 线程被中断")
        } catch (e: IOException) {
            Log.e(TAG, "网络→设备 IO 异常", e)
            onError(e.message ?: "网络→设备 IO 异常")
        }
        Log.d(TAG, "网络→设备 转发线程结束")
    }

    /**
     * VPN fd 唤醒 workaround。
     *
     * VPN fd 的 FileInputStream.read() 在 close() 后不会唤醒阻塞的读线程。
     * 发一个 UDP 空包到 42.42.42.42:4242 触发 read 返回。
     * 该包不会到达网络（tunnel 已关闭）。
     */
    private fun wakeUpReadWorkaround() {
        try {
            val socket = DatagramSocket()
            val address = InetAddress.getByAddress(DUMMY_ADDRESS)
            val packet = DatagramPacket(ByteArray(0), 0, address, DUMMY_PORT)
            socket.send(packet)
            socket.close()
        } catch (e: IOException) {
            // 忽略，此 workaround 尽力而为
        }
    }

    /**
     * IP 包边界输出流。
     *
     * 从 TCP 字节流中恢复 IP 包边界，按完整 IP 包写入 VPN fd。
     * VPN fd 的 write() 需要每次写入一个完整的 IP 包。
     *
     * 核心逻辑：
     * 1. write() 将数据写入 ByteBuffer
     * 2. sink() 尝试从缓冲区提取完整 IP 包
     * 3. sinkPacket()：读 IPv4 版本号 → 读 total_length → 若有完整包则写出
     */
    private class IPPacketOutputStream(
        private val target: OutputStream
    ) : OutputStream() {

        companion object {
            /** IP 包最大长度（16 位字段存储）。 */
            private const val MAX_IP_PACKET_LENGTH = 1 shl 16 // 65536
        }

        /** 缓冲区：2 倍最大包长，确保总有完整包 + 部分包的空间。 */
        private val buffer = ByteBuffer.allocate(2 * MAX_IP_PACKET_LENGTH)

        /** 写入单个字节到缓冲区，然后尝试提取完整 IP 包。 */
        override fun write(b: Int) {
            if (!buffer.hasRemaining()) {
                throw IOException("IPPacketOutputStream 缓冲区已满")
            }
            buffer.put(b.toByte())
            buffer.flip()
            sink()
            buffer.compact()
        }

        /**
         * 批量写入字节到缓冲区，然后尝试提取完整 IP 包。
         *
         * @param b 数据数组
         * @param off 起始偏移
         * @param len 写入长度
         * @throws IOException 长度超过限制或缓冲区空间不足时抛出
         */
        override fun write(b: ByteArray, off: Int, len: Int) {
            if (len > MAX_IP_PACKET_LENGTH) {
                throw IOException("单次写入不能超过一个 IP 包长度")
            }
            if (len > buffer.remaining()) {
                throw IOException("IPPacketOutputStream 缓冲区空间不足: need=$len remaining=${buffer.remaining()}")
            }
            buffer.put(b, off, len)
            buffer.flip()
            sink()
            buffer.compact()
        }

        /** 关闭底层输出流。 */
        override fun close() {
            target.close()
        }

        /** 刷新底层输出流。 */
        override fun flush() {
            target.flush()
        }

        /** 尝试从缓冲区提取所有完整的 IP 包并写出。 */
        private fun sink() {
            while (sinkPacket()) {
                // 继续提取
            }
        }

        /**
         * 尝试提取一个完整 IP 包并写入 target。
         *
         * @return true 如果成功提取并写出了一个包
         */
        private fun sinkPacket(): Boolean {
            val version = readPacketVersion(buffer)
            if (version == -1) {
                return false
            }
            if (version != 4) {
                Log.e("IPPacketOut", "非 IPv4 包，版本=$version，清空缓冲区")
                buffer.clear()
                return false
            }
            val packetLength = readPacketLength(buffer)
            if (packetLength == -1 || packetLength > buffer.remaining()) {
                // 缓冲区中没有完整包
                return false
            }
            target.write(buffer.array(), buffer.arrayOffset() + buffer.position(), packetLength)
            buffer.position(buffer.position() + packetLength)
            return true
        }

        /** 读取缓冲区当前位置的 IP 版本号（首字节高 4 位）。 */
        private fun readPacketVersion(buf: ByteBuffer): Int {
            if (!buf.hasRemaining()) return -1
            val versionAndIHL = buf.get(buf.position())
            return (versionAndIHL.toInt() shr 4) and 0x0F
        }

        /** 读取缓冲区当前位置的 IP 包总长度（偏移 2 处的 16 位无符号值）。 */
        private fun readPacketLength(buf: ByteBuffer): Int {
            if (buf.limit() < buf.position() + 4) return -1
            return buf.getShort(buf.position() + 2).toInt() and 0xFFFF
        }
    }
}
