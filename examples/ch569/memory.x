/* CH569 memory layout.
 *
 * The 128K window at 0x20020000 is split between writable RAMX and the
 * zero-wait ROM code area by `USER_MEM` in InfoFlash. This board has the
 * 64K + 64K split (`USER_MEM = 01b`); the datasheet's default is 96K + 32K.
 * Writes above the RAMX end are dropped and reads echo the bus, so a map that
 * overstates RAMX fails silently. Change RAMX's LENGTH if your board differs.
 */
MEMORY
{
    CODEFLASH (rwx) : ORIGIN = 0x00000000, LENGTH =  448K
    DATAFLASH (rwx) : ORIGIN = 0x00070000, LENGTH =   32K
    RAMS      (rwx) : ORIGIN = 0x20000000, LENGTH =   16K
    RAMX      (rwx) : ORIGIN = 0x20020000, LENGTH =   64K
}

REGION_ALIAS("FLASH", CODEFLASH);

REGION_ALIAS("RAM", RAMS);
REGION_ALIAS("REGION_TEXT", CODEFLASH);
REGION_ALIAS("REGION_RODATA", CODEFLASH);
REGION_ALIAS("REGION_DATA", RAMS);
REGION_ALIAS("REGION_BSS", RAMS);
REGION_ALIAS("REGION_HEAP", RAMS);
REGION_ALIAS("REGION_STACK", RAMS);
