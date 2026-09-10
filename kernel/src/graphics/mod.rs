use crate::FRAMEBUFFER_REQUEST;

pub fn write_pixel(from_position: (u64, u64), to_position: (u64, u64), pixel: (u32, u32, u32)) {
    let (r, g, b) = pixel;
    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            for y in from_position.1..to_position.1 {
                for x in from_position.0..to_position.0 {
                    // Calculate the pixel offset using the framebuffer information we obtained above.
                    // We skip `i` scanlines (pitch is provided in bytes) and add `i * 4` to skip `i` pixels forward.
                    let pixel_offset = y * framebuffer.pitch() + x * 4;

                    // Write 0xFFFFFFFF to the provided pixel offset to fill it white.
                    unsafe {
                        framebuffer
                            .addr()
                            .add(pixel_offset as usize)
                            .cast::<u32>()
                            .write(r << 16 | g << 8 | b)
                    };
                }
            }
        }
    }
}
