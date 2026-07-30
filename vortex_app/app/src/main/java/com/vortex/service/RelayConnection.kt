package com.vortex.service

import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.util.Log
import java.io.DataInputStream
import java.io.IOException

/**
 * 管理与 Relay Server 的 LocalSocket 连接。
 *
 * 通过 ADB 反向端口转发（adb reverse localabstract:vortex tcp:31416）
 * 连接到 PC 端 Relay Server，完成 client_id 握手后双向传输原始 IP 包。
 */
class RelayConnection {

    companion object {
        private const val TAG = "VortexRelayConn"
        /** ADB reverse 映射的 abstract namespace，与 server 端 adb.rs 一致。 */
        private const val LOCAL_ABSTRACT_NAME = "vortex"
    }

    /** ADB 反向隧道的 Unix 域套接字。 */
    private val localSocket = LocalSocket()

    /**
     * 连接到 Relay Server 并完成 client_id 握手。
     *
     * @return 分配的 client_id
     * @throws IOException 连接失败或握手失败时抛出
     */
    fun connect(): Int {
        Log.d(TAG, "连接到 Relay Server...")
        localSocket.connect(
            LocalSocketAddress(LOCAL_ABSTRACT_NAME, LocalSocketAddress.Namespace.ABSTRACT)
        )
        val clientId = DataInputStream(localSocket.inputStream).readInt()
        Log.d(TAG, "已连接 Relay Server，client_id = ${clientId.toLong() and 0xFFFFFFFFL}")
        return clientId
    }

    /**
     * 发送原始 IP 包字节到 Relay Server。
     *
     * @param packet 数据缓冲区
     * @param length 有效数据长度
     * @throws IOException 写入失败时抛出
     */
    fun send(packet: ByteArray, length: Int) {
        localSocket.outputStream.write(packet, 0, length)
    }

    /**
     * 从 Relay Server 接收数据。
     *
     * 返回的数据可能不与 IP 包边界对齐（TCP 流特性），
     * 需要由调用方（IPPacketOutputStream）处理包边界。
     *
     * @param buffer 接收缓冲区
     * @return 读取的字节数，流结束返回 -1
     * @throws IOException 读取失败时抛出
     */
    fun receive(buffer: ByteArray): Int {
        return localSocket.inputStream.read(buffer)
    }

    /**
     * 关闭连接。
     *
     * 先 shutdownInput/Output 中断阻塞的读写，再 close。
     */
    fun close() {
        try {
            if (localSocket.fileDescriptor != null) {
                localSocket.shutdownInput()
                localSocket.shutdownOutput()
            }
            localSocket.close()
        } catch (e: IOException) {
            Log.w(TAG, "关闭 LocalSocket 异常", e)
        }
    }
}
