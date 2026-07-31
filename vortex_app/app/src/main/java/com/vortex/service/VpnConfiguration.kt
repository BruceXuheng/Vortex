package com.vortex.service

import android.os.Parcel
import android.os.Parcelable
import java.net.InetAddress
import java.net.UnknownHostException

/**
 * VPN 配置参数。
 *
 * 封装可通过 ADB Intent 传递的 VPN 参数：DNS 服务器列表、路由规则列表。
 * 对齐 Gnirehtet 设计，空数组表示使用默认值。
 *
 * @property dnsServers DNS 服务器地址列表，空则使用默认 8.8.8.8
 * @property routes CIDR 格式路由规则列表，空则路由 0.0.0.0/0（全局代理）
 */
class VpnConfiguration(
    val dnsServers: Array<InetAddress> = emptyArray(),
    val routes: Array<Cidr> = emptyArray()
) : Parcelable {

    constructor(source: Parcel) : this(
        dnsServers = parseInetAddresses(source),
        routes = source.createTypedArray(Cidr.CREATOR) ?: emptyArray()
    )

    override fun writeToParcel(dest: Parcel, flags: Int) {
        dest.writeInt(dnsServers.size)
        for (addr in dnsServers) {
            dest.writeByteArray(addr.address)
        }
        dest.writeTypedArray(routes, 0)
    }

    override fun describeContents(): Int = 0

    companion object CREATOR : Parcelable.Creator<VpnConfiguration> {
        override fun createFromParcel(source: Parcel): VpnConfiguration =
            VpnConfiguration(source)

        override fun newArray(size: Int): Array<VpnConfiguration?> =
            arrayOfNulls(size)

        /** Intent Extra 键：DNS 服务器列表。 */
        const val EXTRA_DNS_SERVERS = "dnsServers"

        /** Intent Extra 键：路由规则列表。 */
        const val EXTRA_ROUTES = "routes"
    }
}

/**
 * CIDR 格式的路由规则。
 *
 * @property address 网络地址
 * @property prefixLength 前缀长度
 */
class Cidr(
    val address: InetAddress,
    val prefixLength: Int
) : Parcelable {

    constructor(source: Parcel) : this(
        address = InetAddress.getByAddress(source.createByteArray()),
        prefixLength = source.readInt()
    )

    override fun writeToParcel(dest: Parcel, flags: Int) {
        dest.writeByteArray(address.address)
        dest.writeInt(prefixLength)
    }

    override fun describeContents(): Int = 0

    override fun toString(): String = "${address.hostAddress}/$prefixLength"

    companion object CREATOR : Parcelable.Creator<Cidr> {
        override fun createFromParcel(source: Parcel): Cidr = Cidr(source)

        override fun newArray(size: Int): Array<Cidr?> = arrayOfNulls(size)

        /**
         * 从 CIDR 字符串解析路由规则。
         *
         * 支持格式：`192.168.0.0/16` 或 `192.168.0.0`（默认 /32）
         *
         * @param cidr CIDR 格式字符串
         * @return 解析后的 [Cidr]
         * @throws IllegalArgumentException 格式无效时抛出
         */
        fun parse(cidr: String): Cidr {
            val slashIndex = cidr.indexOf('/')
            return try {
                if (slashIndex != -1) {
                    val address = InetAddress.getByName(cidr.substring(0, slashIndex))
                    val prefix = cidr.substring(slashIndex + 1).toInt()
                    Cidr(address, prefix)
                } else {
                    val address = InetAddress.getByName(cidr)
                    Cidr(address, 32)
                }
            } catch (e: UnknownHostException) {
                throw IllegalArgumentException("无效的 CIDR 地址: $cidr", e)
            }
        }
    }
}

/** 从 Parcel 中读取 InetAddress 数组。 */
private fun parseInetAddresses(source: Parcel): Array<InetAddress> {
    val count = source.readInt()
    return Array(count) {
        InetAddress.getByAddress(source.createByteArray())
    }
}
