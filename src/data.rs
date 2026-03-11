use std::sync::LazyLock;

// Static data tables for the C64 Circle Plotter visualizer.
// Ported from the original JavaScript source.

pub const C64W: usize = 320;
pub const C64H: usize = 200;
pub const CHW: usize = 8;
pub const CHH: usize = 8;
pub const COLS: usize = 40;
pub const ROWS: usize = 25;
pub const SCALE: usize = 2;
pub const SPRITE_W: usize = 24;
pub const SPRITE_H: usize = 21;
pub const MUX_H: usize = 21;
pub const MAX_SPR_LINE: usize = 8;
pub const FPS: f64 = 50.0;
#[allow(dead_code)]
pub const SPR_CX: usize = 8;
#[allow(dead_code)]
pub const SPR_CY: usize = 7;
pub const EMPTY_IDX: u8 = 2;
pub const TOTAL_FRAMES: usize = 304;

pub const FADE_CUTOFF: f64 = 0.85;

// Animation phase boundaries
pub const P1_END: usize = 64;
pub const P2_END: usize = 128;
pub const P3_END: usize = 256;
pub const P4_END: usize = 304;

/// 256 characters, each 8 bytes (8x8 bitmap).
pub static CHARSET: [[u8; 8]; 256] = [
    [3, 15, 31, 63, 127, 127, 255, 255],       // 0
    [192, 240, 248, 252, 254, 254, 255, 255],   // 1
    [0, 0, 0, 0, 0, 0, 0, 0],                   // 2
    [1, 7, 15, 31, 63, 63, 127, 127],           // 3
    [240, 248, 252, 254, 254, 255, 255, 255],   // 4
    [0, 0, 0, 0, 0, 0, 128, 128],               // 5
    [0, 3, 7, 15, 31, 31, 63, 63],              // 6
    [248, 252, 254, 255, 255, 255, 255, 255],   // 7
    [0, 0, 0, 0, 0, 128, 192, 224],             // 8
    [0, 1, 3, 7, 15, 15, 31, 31],               // 9
    [252, 254, 255, 255, 255, 255, 255, 255],   // 10
    [0, 0, 0, 128, 192, 224, 224, 240],         // 11
    [0, 0, 1, 3, 7, 15, 15, 31],               // 12
    [60, 255, 255, 255, 255, 255, 255, 255],    // 13
    [0, 0, 128, 192, 224, 224, 240, 240],       // 14
    [0, 0, 0, 1, 3, 7, 7, 15],                 // 15
    [63, 127, 255, 255, 255, 255, 255, 255],    // 16
    [0, 128, 192, 224, 240, 240, 248, 248],     // 17
    [0, 0, 0, 0, 0, 1, 3, 7],                  // 18
    [31, 63, 127, 255, 255, 255, 255, 255],     // 19
    [0, 192, 224, 240, 248, 248, 252, 252],     // 20
    [0, 0, 0, 0, 0, 0, 1, 1],                  // 21
    [15, 31, 63, 127, 127, 255, 255, 255],      // 22
    [128, 224, 240, 248, 252, 252, 254, 254],   // 23
    [255, 127, 127, 63, 31, 15, 3, 0],          // 24
    [255, 254, 254, 252, 248, 240, 192, 0],     // 25
    [127, 63, 63, 31, 15, 7, 1, 0],             // 26
    [255, 255, 255, 254, 252, 248, 224, 0],     // 27
    [128, 0, 0, 0, 0, 0, 0, 0],                 // 28
    [63, 63, 31, 15, 7, 1, 0, 0],               // 29
    [255, 255, 255, 255, 254, 252, 240, 0],     // 30
    [192, 128, 0, 0, 0, 0, 0, 0],               // 31
    [31, 15, 15, 7, 3, 1, 0, 0],                // 32
    [255, 255, 255, 255, 255, 254, 120, 0],     // 33
    [224, 192, 192, 128, 0, 0, 0, 0],           // 34
    [15, 7, 7, 3, 1, 0, 0, 0],                  // 35
    [255, 255, 255, 255, 255, 255, 60, 0],      // 36
    [240, 224, 224, 192, 128, 0, 0, 0],         // 37
    [7, 3, 3, 1, 0, 0, 0, 0],                   // 38
    [255, 255, 255, 255, 255, 127, 30, 0],      // 39
    [240, 240, 224, 224, 192, 128, 0, 0],       // 40
    [3, 1, 0, 0, 0, 0, 0, 0],                   // 41
    [255, 255, 255, 255, 127, 63, 15, 0],       // 42
    [252, 248, 248, 240, 224, 192, 0, 0],       // 43
    [1, 0, 0, 0, 0, 0, 0, 0],                   // 44
    [255, 255, 255, 127, 63, 31, 7, 0],         // 45
    [254, 252, 252, 248, 240, 224, 128, 0],     // 46
    [0, 3, 15, 31, 63, 127, 127, 255],          // 47
    [0, 192, 240, 248, 252, 254, 254, 255],     // 48
    [0, 1, 7, 15, 31, 63, 63, 127],             // 49
    [0, 224, 248, 252, 254, 255, 255, 255],     // 50
    [0, 0, 0, 0, 0, 0, 0, 128],                 // 51
    [0, 0, 3, 7, 15, 31, 31, 63],               // 52
    [0, 240, 252, 254, 255, 255, 255, 255],     // 53
    [0, 0, 0, 0, 0, 0, 128, 192],               // 54
    [0, 120, 254, 255, 255, 255, 255, 255],     // 55
    [0, 0, 0, 0, 128, 192, 192, 224],           // 56
    [0, 60, 255, 255, 255, 255, 255, 255],      // 57
    [0, 0, 0, 0, 1, 3, 3, 7],                   // 58
    [0, 30, 127, 255, 255, 255, 255, 255],      // 59
    [0, 0, 0, 0, 0, 0, 1, 3],                   // 60
    [0, 15, 63, 127, 255, 255, 255, 255],       // 61
    [0, 0, 128, 224, 240, 248, 252, 252],       // 62
    [0, 0, 0, 0, 0, 0, 0, 1],                   // 63
    [0, 7, 31, 63, 127, 255, 255, 255],         // 64
    [0, 128, 224, 240, 248, 252, 252, 254],     // 65
    [255, 255, 127, 127, 63, 31, 15, 3],        // 66
    [255, 255, 254, 254, 252, 248, 240, 192],   // 67
    [127, 127, 63, 63, 31, 15, 7, 1],           // 68
    [255, 255, 255, 255, 254, 252, 248, 224],   // 69
    [128, 128, 0, 0, 0, 0, 0, 0],               // 70
    [63, 63, 31, 31, 15, 7, 3, 0],              // 71
    [255, 255, 255, 255, 255, 254, 252, 248],   // 72
    [31, 31, 15, 15, 7, 3, 1, 0],               // 73
    [255, 255, 255, 255, 255, 255, 254, 252],   // 74
    [255, 255, 255, 255, 255, 255, 255, 60],    // 75
    [255, 255, 255, 255, 255, 255, 127, 63],    // 76
    [248, 248, 240, 240, 224, 192, 128, 0],     // 77
    [255, 255, 255, 255, 255, 127, 63, 31],     // 78
    [252, 252, 248, 248, 240, 224, 192, 0],     // 79
    [1, 1, 0, 0, 0, 0, 0, 0],                   // 80
    [255, 255, 255, 255, 127, 63, 31, 7],       // 81
    [254, 254, 252, 252, 248, 240, 224, 128],   // 82
    [0, 0, 192, 240, 248, 252, 254, 254],       // 83
    [0, 0, 224, 248, 252, 254, 255, 255],       // 84
    [0, 0, 0, 1, 7, 15, 31, 63],               // 85
    [0, 0, 240, 252, 254, 255, 255, 255],       // 86
    [0, 0, 120, 254, 255, 255, 255, 255],       // 87
    [0, 0, 60, 255, 255, 255, 255, 255],        // 88
    [0, 0, 30, 127, 255, 255, 255, 255],        // 89
    [0, 0, 15, 63, 127, 255, 255, 255],         // 90
    [0, 0, 0, 128, 224, 240, 248, 252],         // 91
    [0, 0, 7, 31, 63, 127, 255, 255],           // 92
    [127, 127, 127, 63, 63, 31, 15, 7],         // 93
    [63, 63, 63, 31, 31, 15, 7, 3],             // 94
    [31, 31, 31, 15, 15, 7, 3, 1],              // 95
    [255, 255, 255, 255, 255, 255, 255, 254],   // 96
    [224, 224, 224, 192, 192, 128, 0, 0],       // 97
    [15, 15, 15, 7, 7, 3, 1, 0],                // 98
    [255, 255, 255, 255, 255, 255, 255, 255],   // 99
    [240, 240, 240, 224, 224, 192, 128, 0],     // 100
    [7, 7, 7, 3, 3, 1, 0, 0],                   // 101
    [255, 255, 255, 255, 255, 255, 255, 127],   // 102
    [248, 248, 248, 240, 240, 224, 192, 128],   // 103
    [252, 252, 252, 248, 248, 240, 224, 192],   // 104
    [254, 254, 254, 252, 252, 248, 240, 224],   // 105
    [7, 0, 0, 0, 0, 0, 0, 0],                   // 106
    [224, 0, 0, 0, 0, 0, 0, 0],                 // 107
    [60, 0, 0, 0, 0, 0, 0, 0],                  // 108
    [0, 0, 0, 3, 15, 31, 63, 127],              // 109
    [0, 0, 0, 192, 240, 248, 252, 254],         // 110
    [0, 0, 0, 224, 248, 252, 254, 255],         // 111
    [0, 0, 0, 0, 3, 7, 15, 31],                 // 112
    [0, 0, 0, 240, 252, 254, 255, 255],         // 113
    [0, 0, 0, 0, 0, 3, 7, 15],                  // 114
    [0, 0, 0, 120, 254, 255, 255, 255],         // 115
    [0, 0, 0, 60, 255, 255, 255, 255],          // 116
    [0, 0, 0, 30, 127, 255, 255, 255],          // 117
    [0, 0, 0, 0, 0, 192, 224, 240],             // 118
    [0, 0, 0, 15, 63, 127, 255, 255],           // 119
    [0, 0, 0, 0, 128, 224, 240, 248],           // 120
    [0, 0, 0, 7, 31, 63, 127, 255],             // 121
    [63, 127, 127, 127, 63, 63, 31, 15],        // 122
    [0, 128, 128, 128, 0, 0, 0, 0],             // 123
    [31, 63, 63, 63, 31, 31, 15, 7],            // 124
    [128, 192, 192, 192, 128, 128, 0, 0],       // 125
    [15, 31, 31, 31, 15, 15, 7, 3],             // 126
    [192, 224, 224, 224, 192, 192, 128, 0],     // 127
    [7, 15, 15, 15, 7, 7, 3, 1],                // 128
    [224, 240, 240, 240, 224, 224, 192, 128],   // 129
    [3, 7, 7, 7, 3, 3, 1, 0],                   // 130
    [240, 248, 248, 248, 240, 240, 224, 192],   // 131
    [1, 3, 3, 3, 1, 1, 0, 0],                   // 132
    [248, 252, 252, 252, 248, 248, 240, 224],   // 133
    [0, 0, 1, 1, 1, 0, 0, 0],                   // 134
    [252, 254, 254, 254, 252, 252, 248, 240],   // 135
    [7, 3, 1, 0, 0, 0, 0, 0],                   // 136
    [248, 224, 0, 0, 0, 0, 0, 0],               // 137
    [254, 120, 0, 0, 0, 0, 0, 0],               // 138
    [255, 60, 0, 0, 0, 0, 0, 0],                // 139
    [127, 30, 0, 0, 0, 0, 0, 0],                // 140
    [31, 7, 0, 0, 0, 0, 0, 0],                  // 141
    [0, 0, 0, 0, 224, 248, 252, 254],           // 142
    [0, 0, 0, 0, 240, 252, 254, 255],           // 143
    [0, 0, 0, 0, 120, 254, 255, 255],           // 144
    [0, 0, 0, 0, 60, 255, 255, 255],            // 145
    [0, 0, 0, 0, 30, 127, 255, 255],            // 146
    [0, 0, 0, 0, 15, 63, 127, 255],             // 147
    [0, 0, 0, 0, 7, 31, 63, 127],               // 148
    [127, 127, 255, 255, 255, 127, 127, 63],    // 149
    [254, 254, 255, 255, 255, 254, 254, 252],   // 150
    [63, 63, 127, 127, 127, 63, 63, 31],        // 151
    [31, 31, 63, 63, 63, 31, 31, 15],           // 152
    [128, 128, 192, 192, 192, 128, 128, 0],     // 153
    [15, 15, 31, 31, 31, 15, 15, 7],            // 154
    [192, 192, 224, 224, 224, 192, 192, 128],   // 155
    [7, 7, 15, 15, 15, 7, 7, 3],                // 156
    [224, 224, 240, 240, 240, 224, 224, 192],   // 157
    [3, 3, 7, 7, 7, 3, 3, 1],                   // 158
    [240, 240, 248, 248, 248, 240, 240, 224],   // 159
    [1, 1, 3, 3, 3, 1, 1, 0],                   // 160
    [248, 248, 252, 252, 252, 248, 248, 240],   // 161
    [252, 252, 254, 254, 254, 252, 252, 248],   // 162
    [15, 7, 3, 0, 0, 0, 0, 0],                  // 163
    [248, 240, 224, 128, 0, 0, 0, 0],           // 164
    [252, 248, 224, 0, 0, 0, 0, 0],             // 165
    [254, 252, 240, 0, 0, 0, 0, 0],             // 166
    [255, 254, 120, 0, 0, 0, 0, 0],             // 167
    [255, 255, 60, 0, 0, 0, 0, 0],              // 168
    [255, 127, 30, 0, 0, 0, 0, 0],              // 169
    [127, 63, 15, 0, 0, 0, 0, 0],               // 170
    [224, 192, 128, 0, 0, 0, 0, 0],             // 171
    [63, 31, 7, 0, 0, 0, 0, 0],                 // 172
    [240, 224, 192, 0, 0, 0, 0, 0],             // 173
    [0, 0, 0, 0, 0, 224, 248, 252],             // 174
    [0, 0, 0, 0, 0, 240, 252, 254],             // 175
    [0, 0, 0, 0, 0, 120, 254, 255],             // 176
    [0, 0, 0, 0, 0, 60, 255, 255],              // 177
    [0, 0, 0, 0, 0, 30, 127, 255],              // 178
    [0, 0, 0, 0, 0, 15, 63, 127],               // 179
    [0, 0, 0, 0, 0, 7, 31, 63],                 // 180
    [63, 127, 127, 255, 255, 255, 127, 127],    // 181
    [252, 254, 254, 255, 255, 255, 254, 254],   // 182
    [31, 63, 63, 127, 127, 127, 63, 63],        // 183
    [254, 255, 255, 255, 255, 255, 255, 255],   // 184
    [0, 0, 0, 128, 128, 128, 0, 0],             // 185
    [15, 31, 31, 63, 63, 63, 31, 31],           // 186
    [0, 128, 128, 192, 192, 192, 128, 128],     // 187
    [7, 15, 15, 31, 31, 31, 15, 15],            // 188
    [128, 192, 192, 224, 224, 224, 192, 192],   // 189
    [3, 7, 7, 15, 15, 15, 7, 7],                // 190
    [192, 224, 224, 240, 240, 240, 224, 224],   // 191
    [1, 3, 3, 7, 7, 7, 3, 3],                   // 192
    [224, 240, 240, 248, 248, 248, 240, 240],   // 193
    [0, 1, 1, 3, 3, 3, 1, 1],                   // 194
    [240, 248, 248, 252, 252, 252, 248, 248],   // 195
    [0, 0, 0, 1, 1, 1, 0, 0],                   // 196
    [127, 255, 255, 255, 255, 255, 255, 255],   // 197
    [248, 252, 252, 254, 254, 254, 252, 252],   // 198
    [63, 31, 15, 3, 0, 0, 0, 0],                // 199
    [252, 248, 240, 224, 128, 0, 0, 0],         // 200
    [31, 15, 7, 3, 0, 0, 0, 0],                 // 201
    [254, 252, 248, 224, 0, 0, 0, 0],           // 202
    [255, 254, 252, 240, 0, 0, 0, 0],           // 203
    [255, 255, 254, 120, 0, 0, 0, 0],           // 204
    [255, 255, 255, 60, 0, 0, 0, 0],            // 205
    [255, 127, 63, 15, 0, 0, 0, 0],             // 206
    [127, 63, 31, 7, 0, 0, 0, 0],               // 207
    [0, 0, 0, 0, 0, 0, 0, 240],                 // 208
    [0, 0, 0, 0, 0, 0, 224, 248],               // 209
    [0, 0, 0, 0, 0, 0, 120, 254],               // 210
    [0, 0, 0, 0, 0, 0, 60, 255],                // 211
    [0, 0, 0, 0, 0, 0, 30, 127],                // 212
    [0, 0, 0, 0, 0, 0, 7, 31],                  // 213
    [15, 31, 63, 63, 127, 127, 127, 63],        // 214
    [7, 15, 31, 31, 63, 63, 63, 31],            // 215
    [0, 0, 128, 128, 192, 192, 192, 128],       // 216
    [3, 7, 15, 15, 31, 31, 31, 15],             // 217
    [0, 128, 192, 192, 224, 224, 224, 192],     // 218
    [1, 3, 7, 7, 15, 15, 15, 7],                // 219
    [128, 192, 224, 224, 240, 240, 240, 224],   // 220
    [0, 1, 3, 3, 7, 7, 7, 3],                   // 221
    [192, 224, 240, 240, 248, 248, 248, 240],   // 222
    [0, 0, 1, 1, 3, 3, 3, 1],                   // 223
    [224, 240, 248, 248, 252, 252, 252, 248],   // 224
    [240, 248, 252, 252, 254, 254, 254, 252],   // 225
    [127, 63, 31, 15, 3, 0, 0, 0],              // 226
    [254, 252, 248, 240, 192, 0, 0, 0],         // 227
    [255, 254, 252, 248, 224, 0, 0, 0],         // 228
    [255, 255, 254, 252, 240, 0, 0, 0],         // 229
    [255, 255, 255, 254, 120, 0, 0, 0],         // 230
    [255, 255, 255, 255, 60, 0, 0, 0],          // 231
    [255, 255, 255, 127, 30, 0, 0, 0],          // 232
    [255, 255, 127, 63, 15, 0, 0, 0],           // 233
    [255, 127, 63, 31, 7, 0, 0, 0],             // 234
    [0, 0, 0, 0, 0, 0, 0, 30],                  // 235
    [0, 0, 0, 0, 0, 0, 0, 7],                   // 236
    [7, 15, 31, 63, 63, 127, 127, 127],         // 237
    [3, 7, 15, 31, 31, 63, 63, 63],             // 238
    [1, 3, 7, 15, 15, 31, 31, 31],              // 239
    [0, 0, 128, 192, 192, 224, 224, 224],       // 240
    [0, 1, 3, 7, 7, 15, 15, 15],                // 241
    [0, 128, 192, 224, 224, 240, 240, 240],     // 242
    [0, 0, 1, 3, 3, 7, 7, 7],                   // 243
    [128, 192, 224, 240, 240, 248, 248, 248],   // 244
    [192, 224, 240, 248, 248, 252, 252, 252],   // 245
    [224, 240, 248, 252, 252, 254, 254, 254],   // 246
    [127, 127, 63, 31, 15, 3, 0, 0],            // 247
    [254, 254, 252, 248, 240, 192, 0, 0],       // 248
    [255, 255, 254, 252, 248, 224, 0, 0],       // 249
    [255, 255, 255, 254, 252, 240, 0, 0],       // 250
    [255, 255, 255, 255, 254, 120, 0, 0],       // 251
    [255, 255, 255, 255, 255, 60, 0, 0],        // 252
    [255, 255, 255, 255, 127, 30, 0, 0],        // 253
    [255, 255, 255, 127, 63, 15, 0, 0],         // 254
    [255, 255, 127, 63, 31, 7, 0, 0],           // 255
];

/// Sprite bitmap data: 24x21 pixels, 3 bytes per row = 63 bytes.
pub static SPR_BYTES: [u8; 63] = [
    0x03, 0xc0, 0x00,
    0x0f, 0xf0, 0x00,
    0x1f, 0xf8, 0x00,
    0x3f, 0xfc, 0x00,
    0x7f, 0xfe, 0x00,
    0x7f, 0xfe, 0x00,
    0xff, 0xff, 0x00,
    0xff, 0xff, 0x00,
    0xff, 0xff, 0x00,
    0x7f, 0xfe, 0x00,
    0x7f, 0xfe, 0x00,
    0x3f, 0xfc, 0x00,
    0x1f, 0xf8, 0x00,
    0x0f, 0xf0, 0x00,
    0x03, 0xc0, 0x00,
    0x00, 0x00, 0x00,
    0x00, 0x00, 0x00,
    0x00, 0x00, 0x00,
    0x00, 0x00, 0x00,
    0x00, 0x00, 0x00,
    0x00, 0x00, 0x00,
];

/// A single cell in a stamp: row offset, column offset, and character index.
#[derive(Debug, Clone, Copy)]
pub struct StampCell {
    pub dr: i8,
    pub dc: i8,
    pub ch: u8,
}

impl StampCell {
    const fn new(dr: i8, dc: i8, ch: u8) -> Self {
        Self { dr, dc, ch }
    }
}

/// The 64-entry stamp lookup table.
/// Each entry is a list of `StampCell`s describing which character tiles
/// to place at which offsets.
pub static STAMPS_TABLE: LazyLock<Vec<Vec<StampCell>>> = LazyLock::new(|| {
    vec![
        // Entry 0
        vec![
            StampCell::new(0, 0, 0), StampCell::new(0, 1, 1),
            StampCell::new(1, 0, 24), StampCell::new(1, 1, 25),
        ],
        // Entry 1
        vec![
            StampCell::new(0, 0, 3), StampCell::new(0, 1, 4),
            StampCell::new(1, 0, 26), StampCell::new(1, 1, 27),
        ],
        // Entry 2
        vec![
            StampCell::new(0, 0, 6), StampCell::new(0, 1, 7), StampCell::new(0, 2, 8),
            StampCell::new(1, 0, 29), StampCell::new(1, 1, 30), StampCell::new(1, 2, 31),
        ],
        // Entry 3
        vec![
            StampCell::new(0, 0, 9), StampCell::new(0, 1, 10), StampCell::new(0, 2, 11),
            StampCell::new(1, 0, 32), StampCell::new(1, 1, 33), StampCell::new(1, 2, 34),
        ],
        // Entry 4
        vec![
            StampCell::new(0, 0, 12), StampCell::new(0, 1, 13), StampCell::new(0, 2, 14),
            StampCell::new(1, 0, 35), StampCell::new(1, 1, 36), StampCell::new(1, 2, 37),
        ],
        // Entry 5
        vec![
            StampCell::new(0, 0, 15), StampCell::new(0, 1, 16), StampCell::new(0, 2, 17),
            StampCell::new(1, 0, 38), StampCell::new(1, 1, 39), StampCell::new(1, 2, 40),
        ],
        // Entry 6
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 19), StampCell::new(0, 2, 20),
            StampCell::new(1, 0, 41), StampCell::new(1, 1, 42), StampCell::new(1, 2, 43),
        ],
        // Entry 7
        vec![
            StampCell::new(0, 0, 21), StampCell::new(0, 1, 22), StampCell::new(0, 2, 23),
            StampCell::new(1, 0, 44), StampCell::new(1, 1, 45), StampCell::new(1, 2, 46),
        ],
        // Entry 8
        vec![
            StampCell::new(0, 0, 47), StampCell::new(0, 1, 48),
            StampCell::new(1, 0, 66), StampCell::new(1, 1, 67),
        ],
        // Entry 9
        vec![
            StampCell::new(0, 0, 49), StampCell::new(0, 1, 50),
            StampCell::new(1, 0, 68), StampCell::new(1, 1, 69),
        ],
        // Entry 10
        vec![
            StampCell::new(0, 0, 52), StampCell::new(0, 1, 53), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 71), StampCell::new(1, 1, 72), StampCell::new(1, 2, 34),
        ],
        // Entry 11
        vec![
            StampCell::new(0, 0, 12), StampCell::new(0, 1, 55), StampCell::new(0, 2, 56),
            StampCell::new(1, 0, 73), StampCell::new(1, 1, 74), StampCell::new(1, 2, 37),
        ],
        // Entry 12
        vec![
            StampCell::new(0, 0, 15), StampCell::new(0, 1, 57), StampCell::new(0, 2, 11),
            StampCell::new(1, 0, 32), StampCell::new(1, 1, 75), StampCell::new(1, 2, 40),
        ],
        // Entry 13
        vec![
            StampCell::new(0, 0, 58), StampCell::new(0, 1, 59), StampCell::new(0, 2, 14),
            StampCell::new(1, 0, 35), StampCell::new(1, 1, 76), StampCell::new(1, 2, 77),
        ],
        // Entry 14
        vec![
            StampCell::new(0, 0, 60), StampCell::new(0, 1, 61), StampCell::new(0, 2, 62),
            StampCell::new(1, 0, 38), StampCell::new(1, 1, 78), StampCell::new(1, 2, 79),
        ],
        // Entry 15
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 64), StampCell::new(0, 2, 65),
            StampCell::new(1, 0, 80), StampCell::new(1, 1, 81), StampCell::new(1, 2, 82),
        ],
        // Entry 16
        vec![
            StampCell::new(0, 0, 49), StampCell::new(0, 1, 83),
            StampCell::new(1, 0, 81), StampCell::new(1, 1, 69),
            StampCell::new(2, 0, 106), StampCell::new(2, 1, 31),
        ],
        // Entry 17
        vec![
            StampCell::new(0, 0, 52), StampCell::new(0, 1, 84),
            StampCell::new(1, 0, 93), StampCell::new(1, 1, 72),
            StampCell::new(2, 0, 44), StampCell::new(2, 1, 107),
        ],
        // Entry 18
        vec![
            StampCell::new(0, 0, 85), StampCell::new(0, 1, 86), StampCell::new(0, 2, 5),
            StampCell::new(1, 0, 94), StampCell::new(1, 1, 74), StampCell::new(1, 2, 34),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 107), StampCell::new(2, 2, 2),
        ],
        // Entry 19
        vec![
            StampCell::new(0, 0, 15), StampCell::new(0, 1, 87), StampCell::new(0, 2, 8),
            StampCell::new(1, 0, 95), StampCell::new(1, 1, 96), StampCell::new(1, 2, 97),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 108), StampCell::new(2, 2, 2),
        ],
        // Entry 20
        vec![
            StampCell::new(0, 0, 58), StampCell::new(0, 1, 88), StampCell::new(0, 2, 56),
            StampCell::new(1, 0, 98), StampCell::new(1, 1, 99), StampCell::new(1, 2, 100),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 108), StampCell::new(2, 2, 2),
        ],
        // Entry 21
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 89), StampCell::new(0, 2, 11),
            StampCell::new(1, 0, 101), StampCell::new(1, 1, 102), StampCell::new(1, 2, 103),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 108), StampCell::new(2, 2, 2),
        ],
        // Entry 22
        vec![
            StampCell::new(0, 0, 21), StampCell::new(0, 1, 90), StampCell::new(0, 2, 91),
            StampCell::new(1, 0, 38), StampCell::new(1, 1, 76), StampCell::new(1, 2, 104),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 106), StampCell::new(2, 2, 2),
        ],
        // Entry 23
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 92), StampCell::new(0, 2, 62),
            StampCell::new(1, 0, 80), StampCell::new(1, 1, 78), StampCell::new(1, 2, 105),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 106), StampCell::new(2, 2, 28),
        ],
        // Entry 24
        vec![
            StampCell::new(0, 0, 109), StampCell::new(0, 1, 110),
            StampCell::new(1, 0, 78), StampCell::new(1, 1, 72),
            StampCell::new(2, 0, 136), StampCell::new(2, 1, 137),
        ],
        // Entry 25
        vec![
            StampCell::new(0, 0, 85), StampCell::new(0, 1, 111),
            StampCell::new(1, 0, 122), StampCell::new(1, 1, 74),
            StampCell::new(2, 0, 106), StampCell::new(2, 1, 137),
        ],
        // Entry 26
        vec![
            StampCell::new(0, 0, 112), StampCell::new(0, 1, 113), StampCell::new(0, 2, 51),
            StampCell::new(1, 0, 124), StampCell::new(1, 1, 96), StampCell::new(1, 2, 125),
            StampCell::new(2, 0, 106), StampCell::new(2, 1, 137), StampCell::new(2, 2, 2),
        ],
        // Entry 27
        vec![
            StampCell::new(0, 0, 114), StampCell::new(0, 1, 115), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 126), StampCell::new(1, 1, 99), StampCell::new(1, 2, 127),
            StampCell::new(2, 0, 44), StampCell::new(2, 1, 138), StampCell::new(2, 2, 2),
        ],
        // Entry 28
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 116), StampCell::new(0, 2, 8),
            StampCell::new(1, 0, 128), StampCell::new(1, 1, 99), StampCell::new(1, 2, 129),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 139), StampCell::new(2, 2, 2),
        ],
        // Entry 29
        vec![
            StampCell::new(0, 0, 60), StampCell::new(0, 1, 117), StampCell::new(0, 2, 118),
            StampCell::new(1, 0, 130), StampCell::new(1, 1, 99), StampCell::new(1, 2, 131),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 140), StampCell::new(2, 2, 28),
        ],
        // Entry 30
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 119), StampCell::new(0, 2, 120),
            StampCell::new(1, 0, 132), StampCell::new(1, 1, 102), StampCell::new(1, 2, 133),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 141), StampCell::new(2, 2, 31),
        ],
        // Entry 31
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 121), StampCell::new(0, 2, 91),
            StampCell::new(1, 0, 134), StampCell::new(1, 1, 76), StampCell::new(1, 2, 135),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 141), StampCell::new(2, 2, 31),
        ],
        // Entry 32
        vec![
            StampCell::new(0, 0, 85), StampCell::new(0, 1, 91),
            StampCell::new(1, 0, 149), StampCell::new(1, 1, 150),
            StampCell::new(2, 0, 163), StampCell::new(2, 1, 164),
        ],
        // Entry 33
        vec![
            StampCell::new(0, 0, 112), StampCell::new(0, 1, 142),
            StampCell::new(1, 0, 151), StampCell::new(1, 1, 96),
            StampCell::new(2, 0, 163), StampCell::new(2, 1, 165),
        ],
        // Entry 34
        vec![
            StampCell::new(0, 0, 114), StampCell::new(0, 1, 143), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 152), StampCell::new(1, 1, 99), StampCell::new(1, 2, 153),
            StampCell::new(2, 0, 136), StampCell::new(2, 1, 166), StampCell::new(2, 2, 2),
        ],
        // Entry 35
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 144), StampCell::new(0, 2, 51),
            StampCell::new(1, 0, 154), StampCell::new(1, 1, 99), StampCell::new(1, 2, 155),
            StampCell::new(2, 0, 41), StampCell::new(2, 1, 167), StampCell::new(2, 2, 2),
        ],
        // Entry 36
        vec![
            StampCell::new(0, 0, 60), StampCell::new(0, 1, 145), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 156), StampCell::new(1, 1, 99), StampCell::new(1, 2, 157),
            StampCell::new(2, 0, 44), StampCell::new(2, 1, 168), StampCell::new(2, 2, 28),
        ],
        // Entry 37
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 146), StampCell::new(0, 2, 8),
            StampCell::new(1, 0, 158), StampCell::new(1, 1, 99), StampCell::new(1, 2, 159),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 169), StampCell::new(2, 2, 31),
        ],
        // Entry 38
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 147), StampCell::new(0, 2, 118),
            StampCell::new(1, 0, 160), StampCell::new(1, 1, 99), StampCell::new(1, 2, 161),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 170), StampCell::new(2, 2, 171),
        ],
        // Entry 39
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 148), StampCell::new(0, 2, 120),
            StampCell::new(1, 0, 134), StampCell::new(1, 1, 102), StampCell::new(1, 2, 162),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 172), StampCell::new(2, 2, 173),
        ],
        // Entry 40
        vec![
            StampCell::new(0, 0, 114), StampCell::new(0, 1, 120),
            StampCell::new(1, 0, 181), StampCell::new(1, 1, 182),
            StampCell::new(2, 0, 199), StampCell::new(2, 1, 200),
        ],
        // Entry 41
        vec![
            StampCell::new(0, 0, 114), StampCell::new(0, 1, 174),
            StampCell::new(1, 0, 183), StampCell::new(1, 1, 184),
            StampCell::new(2, 0, 201), StampCell::new(2, 1, 202),
        ],
        // Entry 42
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 175), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 186), StampCell::new(1, 1, 99), StampCell::new(1, 2, 187),
            StampCell::new(2, 0, 163), StampCell::new(2, 1, 203), StampCell::new(2, 2, 2),
        ],
        // Entry 43
        vec![
            StampCell::new(0, 0, 60), StampCell::new(0, 1, 176), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 188), StampCell::new(1, 1, 99), StampCell::new(1, 2, 189),
            StampCell::new(2, 0, 136), StampCell::new(2, 1, 204), StampCell::new(2, 2, 28),
        ],
        // Entry 44
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 177), StampCell::new(0, 2, 51),
            StampCell::new(1, 0, 190), StampCell::new(1, 1, 99), StampCell::new(1, 2, 191),
            StampCell::new(2, 0, 41), StampCell::new(2, 1, 205), StampCell::new(2, 2, 31),
        ],
        // Entry 45
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 178), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 192), StampCell::new(1, 1, 99), StampCell::new(1, 2, 193),
            StampCell::new(2, 0, 44), StampCell::new(2, 1, 205), StampCell::new(2, 2, 171),
        ],
        // Entry 46
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 179), StampCell::new(0, 2, 8),
            StampCell::new(1, 0, 194), StampCell::new(1, 1, 99), StampCell::new(1, 2, 195),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 206), StampCell::new(2, 2, 173),
        ],
        // Entry 47
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 180), StampCell::new(0, 2, 118),
            StampCell::new(1, 0, 196), StampCell::new(1, 1, 197), StampCell::new(1, 2, 198),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 207), StampCell::new(2, 2, 164),
        ],
        // Entry 48
        vec![
            StampCell::new(0, 0, 18), StampCell::new(0, 1, 208),
            StampCell::new(1, 0, 19), StampCell::new(1, 1, 7),
            StampCell::new(2, 0, 226), StampCell::new(2, 1, 227),
        ],
        // Entry 49
        vec![
            StampCell::new(0, 0, 60), StampCell::new(0, 1, 209),
            StampCell::new(1, 0, 214), StampCell::new(1, 1, 10),
            StampCell::new(2, 0, 199), StampCell::new(2, 1, 228),
        ],
        // Entry 50
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 209), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 215), StampCell::new(1, 1, 184), StampCell::new(1, 2, 216),
            StampCell::new(2, 0, 201), StampCell::new(2, 1, 229), StampCell::new(2, 2, 28),
        ],
        // Entry 51
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 210), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 217), StampCell::new(1, 1, 99), StampCell::new(1, 2, 218),
            StampCell::new(2, 0, 163), StampCell::new(2, 1, 230), StampCell::new(2, 2, 31),
        ],
        // Entry 52
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 211), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 219), StampCell::new(1, 1, 99), StampCell::new(1, 2, 220),
            StampCell::new(2, 0, 136), StampCell::new(2, 1, 231), StampCell::new(2, 2, 171),
        ],
        // Entry 53
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 212), StampCell::new(0, 2, 51),
            StampCell::new(1, 0, 221), StampCell::new(1, 1, 99), StampCell::new(1, 2, 222),
            StampCell::new(2, 0, 41), StampCell::new(2, 1, 232), StampCell::new(2, 2, 173),
        ],
        // Entry 54
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 213), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 223), StampCell::new(1, 1, 197), StampCell::new(1, 2, 224),
            StampCell::new(2, 0, 44), StampCell::new(2, 1, 233), StampCell::new(2, 2, 164),
        ],
        // Entry 55
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 213), StampCell::new(0, 2, 54),
            StampCell::new(1, 0, 196), StampCell::new(1, 1, 16), StampCell::new(1, 2, 225),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 234), StampCell::new(2, 2, 200),
        ],
        // Entry 56
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 54),
            StampCell::new(1, 0, 22), StampCell::new(1, 1, 4),
            StampCell::new(2, 0, 247), StampCell::new(2, 1, 248),
        ],
        // Entry 57
        vec![
            StampCell::new(0, 0, 63), StampCell::new(0, 1, 208),
            StampCell::new(1, 0, 237), StampCell::new(1, 1, 7),
            StampCell::new(2, 0, 29), StampCell::new(2, 1, 249),
        ],
        // Entry 58
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 208), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 238), StampCell::new(1, 1, 10), StampCell::new(1, 2, 56),
            StampCell::new(2, 0, 32), StampCell::new(2, 1, 250), StampCell::new(2, 2, 70),
        ],
        // Entry 59
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 208), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 239), StampCell::new(1, 1, 184), StampCell::new(1, 2, 240),
            StampCell::new(2, 0, 35), StampCell::new(2, 1, 251), StampCell::new(2, 2, 171),
        ],
        // Entry 60
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 235), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 241), StampCell::new(1, 1, 99), StampCell::new(1, 2, 242),
            StampCell::new(2, 0, 38), StampCell::new(2, 1, 252), StampCell::new(2, 2, 34),
        ],
        // Entry 61
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 235), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 243), StampCell::new(1, 1, 197), StampCell::new(1, 2, 244),
            StampCell::new(2, 0, 136), StampCell::new(2, 1, 253), StampCell::new(2, 2, 37),
        ],
        // Entry 62
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 236), StampCell::new(0, 2, 2),
            StampCell::new(1, 0, 58), StampCell::new(1, 1, 16), StampCell::new(1, 2, 245),
            StampCell::new(2, 0, 80), StampCell::new(2, 1, 254), StampCell::new(2, 2, 200),
        ],
        // Entry 63
        vec![
            StampCell::new(0, 0, 2), StampCell::new(0, 1, 236), StampCell::new(0, 2, 51),
            StampCell::new(1, 0, 21), StampCell::new(1, 1, 19), StampCell::new(1, 2, 246),
            StampCell::new(2, 0, 2), StampCell::new(2, 1, 255), StampCell::new(2, 2, 43),
        ],
    ]
});

/// Pre-expanded sprite bitmap as a pixel array (lazily computed once).
/// Each entry is 1 (set) or 0 (clear), row-major order.
pub static SPR_PIXELS: LazyLock<[u8; SPRITE_W * SPRITE_H]> = LazyLock::new(|| {
    let mut pixels = [0u8; SPRITE_W * SPRITE_H];
    for row in 0..SPRITE_H {
        for byte_col in 0..3 {
            let b = SPR_BYTES[row * 3 + byte_col];
            for bit in 0..8 {
                if b & (0x80 >> bit) != 0 {
                    pixels[row * SPRITE_W + byte_col * 8 + bit] = 1;
                }
            }
        }
    }
    pixels
});

/// Pre-expanded charset characters as pixel arrays (lazily computed once).
/// Each character produces 64 pixels (8x8), row-major, with values 1 or 0.
pub static CHAR_PIXELS: LazyLock<[[u8; 64]; 256]> = LazyLock::new(|| {
    let mut all = [[0u8; 64]; 256];
    for ch in 0..256 {
        for row in 0..8 {
            let b = CHARSET[ch][row];
            for bit in 0..8 {
                if b & (0x80 >> bit) != 0 {
                    all[ch][row * 8 + bit] = 1;
                }
            }
        }
    }
    all
});

