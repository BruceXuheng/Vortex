package com.vortex.data

/**
 * 流量日志数据类。
 *
 * @param id 日志唯一标识
 * @param source 流量来源地址
 * @param destination 流量目标地址
 * @param timestamp 日志时间戳
 */
data class TrafficLog(
    val id: Int,
    val source: String,
    val destination: String,
    val timestamp: String
)
