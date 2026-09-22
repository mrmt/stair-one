// engine/ffi (Rust) の C ABI
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct StairHandle StairHandle;

StairHandle* stair_new(double sample_rate, uint32_t seed);
void stair_free(StairHandle* h);
void stair_note_on(StairHandle* h, uint32_t pad);
void stair_note_off(StairHandle* h, uint32_t pad);
void stair_set_param(StairHandle* h, uint32_t idx, double value);
void stair_render(StairHandle* h, float* l, float* r, uint32_t n);
uint32_t stair_voice_count(StairHandle* h);
uint32_t stair_pad_count(void);
uint32_t stair_param_count(void);
uint32_t stair_param_def(uint32_t idx, char* id, char* label, uint32_t cap,
                         double* min, double* max, double* step, double* def);

#ifdef __cplusplus
}
#endif
