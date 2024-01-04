// Taken from RTR vol 4 pg. 278
const RGB_TO_LUM = vec3<f32>(0.2125, 0.7154, 0.0721);

struct Params {
    min_log_lum: f32,
    inv_log_lum_range: f32,
    log_lum_range: f32,
    num_pixels: f32,
    delta_t: f32,
    correction: f32,
}

@group(0) @binding(0)
var<uniform> params: Params;
@group(0) @binding(1)
var tex_color: texture_2d<f32>;
@group(0) @binding(2)
var tex_mask: texture_2d<f32>;
@group(0) @binding(3)
var<storage, read_write> histogram: array<atomic<u32>, 256>;
@group(0) @binding(4)
var<storage, read_write> result: vec4<f32>;

// Shared histogram buffer used for storing intermediate sums for each work group
var<workgroup> histogram_shared: array<atomic<u32>, 256>;
var<workgroup> total_shared: array<u32, 256>;

// For a given color and luminance range, return the histogram bin index
fn colorToBin(hdrColor: vec3<f32>, minLogLum: f32, inverseLogLumRange: f32) -> u32 {
    // Convert our RGB value to Luminance, see note for RGB_TO_LUM macro above
    let lum = dot(hdrColor, RGB_TO_LUM);

    // Avoid taking the log of zero
    if lum < exp2(minLogLum) {
        return 0u;
    }

    // Calculate the log_2 luminance and express it as a value in [0.0, 1.0]
    // where 0.0 represents the minimum luminance, and 1.0 represents the max.
    let logLum = saturate((log2(lum) - minLogLum) * inverseLogLumRange);

    // Map [0, 1] to [1, 255]. The zeroth bin is handled by the epsilon check above.
    return u32(logLum * 254.0 + 1.0);
}

@compute @workgroup_size(16, 16, 1)
fn computeHistogram(
    @builtin(global_invocation_id) global_invocation_id: vec3<u32>,
    @builtin(local_invocation_index) local_invocation_index: u32
) {
    // Initialize the bin for this thread to 0
    histogram_shared[local_invocation_index] = 0u;
    storageBarrier();

    let dim = vec2<u32>(textureDimensions(tex_color));
    let uv = vec2<f32>(global_invocation_id.xy) / vec2<f32>(dim);

    // Ignore threads that map to areas beyond the bounds of our HDR image
    if global_invocation_id.x < dim.x && global_invocation_id.y < dim.y {
        let hdrColor = textureLoad(tex_color, vec2<i32>(global_invocation_id.xy), 0).rgb;
        let binIndex = colorToBin(hdrColor, params.min_log_lum, params.inv_log_lum_range);

        let exposureMask = textureLoad(tex_mask, vec2<i32>(uv * vec2<f32>(textureDimensions(tex_mask))), 0).r;
        let weightedContribution = u32(exposureMask * 8.0);

        // We use an atomic add to ensure we don't write to the same bin in our
        // histogram from two different threads at the same time.
        atomicAdd(&histogram_shared[binIndex], weightedContribution);
    }

    // Wait for all threads in the work group to reach this point before adding our
    // local histogram to the global one
    workgroupBarrier();

    // Technically there's no chance that two threads write to the same bin here,
    // but different work groups might! So we still need the atomic add.
    atomicAdd(&histogram[local_invocation_index], histogram_shared[local_invocation_index]);
}

@compute @workgroup_size(1, 1, 1)
fn computeAverage(@builtin(local_invocation_index) local_index: u32) {
    var histogram_sum = 0u;
    for (var i=0u; i<256u; i+=1u) {
        histogram_sum += histogram[i];
        histogram_shared[i] = histogram_sum;
        histogram[i] = 0u;
    }

    let first_index = histogram_sum * 70u / 100u;
    let last_index = histogram_sum * 95u / 100u;

    var count = 0u;
    var sum = 0.0;
    for (var i=0u; i<256u; i+=1u) {
        let bin_count =
            clamp(histogram_shared[i], first_index, last_index) -
            clamp(histogram_shared[i - 1u], first_index, last_index);

        sum += f32(bin_count) * (params.min_log_lum + f32(i) / 255.0 * params.log_lum_range);
        count += bin_count;
    }

    var target_exposure = 0.0;

    if count > 0u {
        target_exposure = log2(1.2) - sum / f32(count);
    }

    let lum_change = target_exposure - result.x;
    let lum_rate = min(abs(lum_change), params.delta_t);
    let lum_delta = sign(lum_change) * lum_rate;
    result = vec4(result.x + lum_delta, 1.0, 1.0, 1.0);

}
