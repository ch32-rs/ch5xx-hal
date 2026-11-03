/* Reserved DMA destinations and their possible truncated-address aliases.
 * NOLOAD regions are initialized explicitly by the acceptance firmware. */
SECTIONS {
  .dma_probe_a 0x20003000 (NOLOAD) : { KEEP(*(.dma_probe_a)) } > RAM
  .dma_probe_b 0x20007000 (NOLOAD) : { KEEP(*(.dma_probe_b)) } > RAM
  .dma_probe_c 0x2000b000 (NOLOAD) : { KEEP(*(.dma_probe_c)) } > RAM
  .dma_probe_d 0x20013000 (NOLOAD) : { KEEP(*(.dma_probe_d)) } > RAM
} INSERT AFTER .uninit;
