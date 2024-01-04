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
        let weightedContribution = u32(exposureMask * 1.0);

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

@compute @workgroup_size(256, 1, 1)
fn computeAverage(@builtin(local_invocation_index) local_index: u32) {
    // Get the count from the histogram buffer
    let count = histogram[local_index];
    total_shared[local_index] = count;
    histogram_shared[local_index] = count * local_index;

    storageBarrier();

    // Reset the count stored in the buffer in anticipation of the next pass
    histogram[local_index] = 0u;

    // This loop will perform a weighted count of the luminance range
    for (var cutoff = 128u; cutoff > 0u; cutoff >>= 1u) {
        if u32(local_index) < cutoff {
            histogram_shared[local_index] += histogram_shared[local_index + cutoff];
            total_shared[local_index] += total_shared[local_index + cutoff];
        }
        workgroupBarrier();
    }

    // We only need to calculate this once, so only a single thread is needed.
    if local_index == 0u {
        // Here we take our weighted sum and divide it by the number of pixels
        // that had luminance greater than zero (since the index == 0, we can
        // use countForThisBin to find the number of black pixels)
        let weighted_log_average = (f32(histogram_shared[0]) / max(f32(total_shared[0]) - f32(count), 1.0)) - 1.0;

        // Map from our histogram space to actual luminance
        let weighted_avg_lum = ((weighted_log_average / 254.0) * params.log_lum_range) + params.min_log_lum;

        // The new stored value will be interpolated using the last frames value
        // to prevent sudden shifts in the exposure.
        let target_exposure = log2(1.2) - weighted_avg_lum;

        let lum_change = target_exposure - result.x;
        let lum_rate = min(abs(lum_change), params.delta_t);
        let lum_delta = sign(lum_change) * lum_rate;
        result = vec4(result.x + lum_delta, 1.0, 1.0, 1.0);
    }
}
