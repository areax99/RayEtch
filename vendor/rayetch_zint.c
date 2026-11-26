/* RayEtch's bridge to Zint: encode one symbol and hand back its modules as a byte grid, so the
 * Rust side never depends on the layout of Zint's structures. */
#include <stdlib.h>
#include <string.h>
#include "zint/zint.h"

/* Encode `data` (len bytes) as `symbology` with Zint's option_1/2/3 and input mode.
 * On success: rows × width module bytes (1 = dark) in `modules` (capacity `cap`), each row's
 * height in modules in `heights` (capacity 200), the human-readable text in `text` (cap 256)
 * and 0 (or a Zint warning below 5) is returned. On failure: Zint's error number and its
 * message in `err` (cap 128). */
int rayetch_zint_encode(int symbology, const unsigned char *data, int len, int option_1, int option_2,
                        int option_3, int input_mode, unsigned char *modules, int cap, int *rows, int *width,
                        float *heights, char *text, char *err) {
    struct zint_symbol *s = ZBarcode_Create();
    int r, c, ret;
    if (!s) {
        strcpy(err, "out of memory");
        return 99;
    }
    s->symbology = symbology;
    s->option_1 = option_1;
    s->option_2 = option_2;
    s->option_3 = option_3;
    s->input_mode = input_mode;
    s->show_hrt = 1;
    ret = ZBarcode_Encode(s, data, len);
    err[0] = 0;
    text[0] = 0;
    if (ret >= ZINT_ERROR) {
        strncpy(err, s->errtxt, 127);
        err[127] = 0;
        ZBarcode_Delete(s);
        return ret;
    }
    *rows = s->rows;
    *width = s->width;
    if ((long)s->rows * s->width > cap || s->rows > 200) {
        strcpy(err, "symbol too large");
        ZBarcode_Delete(s);
        return 98;
    }
    for (r = 0; r < s->rows; r++) {
        heights[r] = s->row_height[r];
        for (c = 0; c < s->width; c++) {
            modules[r * s->width + c] = (s->encoded_data[r][c >> 3] >> (c & 7)) & 1;
        }
    }
    strncpy(text, (const char *)s->text, 255);
    text[255] = 0;
    ZBarcode_Delete(s);
    return ret;
}
