#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

int main(void)
{
    printf("OCTAGRAM-OS POSIX Framebuffer Demo\n");

    int fd = open("/dev/fb0", O_RDWR);
    if (fd < 0) {
        printf("Error: cannot open /dev/fb0\n");
        return 1;
    }

    struct fb_var_screeninfo vinfo;
    if (ioctl(fd, FBIOGET_VSCREENINFO, &vinfo) < 0) {
        printf("Error: ioctl FBIOGET_VSCREENINFO failed\n");
        close(fd);
        return 1;
    }

    struct fb_fix_screeninfo finfo;
    if (ioctl(fd, FBIOGET_FSCREENINFO, &finfo) < 0) {
        printf("Error: ioctl FBIOGET_FSCREENINFO failed\n");
        close(fd);
        return 1;
    }

    printf("Resolution : %ux%u (%u bpp)\n", vinfo.xres, vinfo.yres, vinfo.bits_per_pixel);
    printf("Line length: %u bytes (smem_len: %u bytes)\n", finfo.line_length, finfo.smem_len);

    size_t screensize = (size_t)finfo.smem_len;
    uint32_t *fb = (uint32_t *)mmap(NULL, screensize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (fb == MAP_FAILED) {
        printf("Error: mmap /dev/fb0 failed\n");
        close(fd);
        return 1;
    }

    uint32_t width = vinfo.xres;
    uint32_t height = vinfo.yres;
    uint32_t stride = finfo.line_length / 4;

    // 1. Draw gradient background
    for (uint32_t y = 0; y < height; ++y) {
        uint32_t r = (y * 64) / height;
        uint32_t g = (y * 32) / height;
        uint32_t b = 128 + (y * 127) / height;
        uint32_t color = (r << 16) | (g << 8) | b;
        for (uint32_t x = 0; x < width; ++x) {
            fb[y * stride + x] = color;
        }
    }

    // 2. Draw color bars in the center
    uint32_t colors[] = {
        0x00FF0000, // Red
        0x0000FF00, // Green
        0x000000FF, // Blue
        0x00FFFF00, // Yellow
        0x0000FFFF, // Cyan
        0x00FF00FF, // Magenta
        0x00FFFFFF, // White
        0x00000000, // Black
    };
    int num_colors = sizeof(colors) / sizeof(colors[0]);
    uint32_t bar_width = width / (num_colors + 2);
    uint32_t bar_height = height / 3;
    uint32_t bar_y = height / 3;

    for (int i = 0; i < num_colors; ++i) {
        uint32_t start_x = (i + 1) * bar_width;
        uint32_t c = colors[i];
        for (uint32_t y = bar_y; y < bar_y + bar_height; ++y) {
            for (uint32_t x = start_x; x < start_x + bar_width; ++x) {
                fb[y * stride + x] = c;
            }
        }
    }

    // 3. Draw a framed central box
    uint32_t box_w = width / 2;
    uint32_t box_h = height / 6;
    uint32_t box_x = width / 4;
    uint32_t box_y = (height * 3) / 4;

    for (uint32_t y = box_y; y < box_y + box_h; ++y) {
        for (uint32_t x = box_x; x < box_x + box_w; ++x) {
            if (x == box_x || x == box_x + box_w - 1 || y == box_y || y == box_y + box_h - 1) {
                fb[y * stride + x] = 0x00FFFFFF; // White border
            } else {
                fb[y * stride + x] = 0x001A1A2E; // Dark navy inside
            }
        }
    }

    printf("Rendered POSIX graphics via /dev/fb0 mmap successfully!\n");

    munmap(fb, screensize);
    close(fd);
    return 0;
}
