// Measure one generic MLX D512 GPU dispatch per Metal command buffer.
import Foundation
import Metal

let args = CommandLine.arguments
let dtype = args.count == 6 ? args[5] : "bf16"
guard (5...6).contains(args.count), ["bf16", "fp32"].contains(dtype), let tokens = Int32(args[3]), tokens > 0,
      let blocks = Int32(args[4]), blocks > 0, blocks % 32 == 0,
      let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
    fatalError("Usage: sdpa-first-pass BASE.metallib PATCHED.metallib KV_LENGTH BLOCKS [bf16|fp32]")
}

let scalar = dtype == "fp32" ? "float" : "bfloat16_t"
let scalarBytes = dtype == "fp32" ? 4 : 2
let name = "sdpa_vector_2pass_1_\(scalar)_512_512"
let constants = MTLFunctionConstantValues()
var disabled = false
for index in 20...25 {
    constants.setConstantValue(&disabled, type: .bool, index: index)
}
var blockCount = blocks
var window: Int32 = 0
constants.setConstantValue(&blockCount, type: .int, index: 26)
constants.setConstantValue(&window, type: .int, index: 27)
let pipelines = try args[1...2].map {
    let library = try device.makeLibrary(URL: URL(fileURLWithPath: $0))
    let function = try library.makeFunction(name: name, constantValues: constants)
    return try device.makeComputePipelineState(function: function)
}

let heads = 32
let kvHeads = 4
let width = 512
let parts = heads * Int(blocks)
let buffers = [heads * width * scalarBytes, kvHeads * Int(tokens) * width * scalarBytes,
               kvHeads * Int(tokens) * width * scalarBytes, parts * width * scalarBytes, parts * 4, parts * 4]
    .map { device.makeBuffer(length: $0, options: .storageModeShared)! }
var seed: UInt32 = 4941
for input in buffers.prefix(3) {
    let count = input.length / scalarBytes
    let values = input.contents().bindMemory(to: UInt16.self, capacity: count * scalarBytes / 2)
    for i in 0..<count {
        seed = 1664525 &* seed &+ 1013904223
        let bits = UInt16(0x3e00 | ((seed >> 16) & 0x7f) | (seed & 0x8000))
        values[i * scalarBytes / 2 + scalarBytes / 2 - 1] = bits
        if dtype == "fp32" { values[i * 2] = 0 }
    }
}
var count = tokens
var headStride = Int(tokens) * width
var sequenceStride = width
var scale = Float(1.0 / sqrt(Double(width)))

func dispatch(_ pipeline: MTLComputePipelineState) -> Double {
    let command = queue.makeCommandBuffer()!
    let encoder = command.makeComputeCommandEncoder()!
    encoder.setComputePipelineState(pipeline)
    for (index, buffer) in buffers.enumerated() {
        encoder.setBuffer(buffer, offset: 0, index: index)
    }
    encoder.setBytes(&count, length: MemoryLayout<Int32>.size, index: 7)
    for index in [8, 10] {
        encoder.setBytes(&headStride, length: MemoryLayout<Int>.size, index: index)
    }
    for index in [9, 11] {
        encoder.setBytes(&sequenceStride, length: MemoryLayout<Int>.size, index: index)
    }
    encoder.setBytes(&scale, length: MemoryLayout<Float>.size, index: 12)
    encoder.dispatchThreadgroups(MTLSize(width: kvHeads, height: 1, depth: Int(blocks)),
                                 threadsPerThreadgroup: MTLSize(width: 32, height: 8, depth: 1))
    encoder.endEncoding()
    command.commit()
    command.waitUntilCompleted()
    guard command.status == .completed, command.gpuEndTime > command.gpuStartTime else {
        fatalError("Missing GPU timestamps: \(String(describing: command.error))")
    }
    return (command.gpuEndTime - command.gpuStartTime) * 1000
}

for pipeline in pipelines {
    for _ in 0..<5 { _ = dispatch(pipeline) }
}
var samples = [[Double]]()
for arm in [0, 1, 1, 0] {
    samples.append((0..<20).map { _ in dispatch(pipelines[arm]) })
}
let report: [String: Any] = ["device": device.name, "kernel": name, "kvLength": tokens,
                              "blocks": blocks, "order": ["base", "patched", "patched", "base"],
                              "gpuCommandBufferMs": samples]
let json = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
print(String(decoding: json, as: UTF8.self))
