/// <reference types="vite/client" />

declare module 'virtual:cuter-image-processors' {
  const processors: {
    MAX_GRID_AXIS: number;
    MAX_GRID_TILES: number;
    createGridTiles(
      width: number,
      height: number,
      rows: number,
      columns: number,
    ): Array<{
      row: number;
      column: number;
      x: number;
      y: number;
      width: number;
      height: number;
    }>;
    splitImage(
      blob: Blob,
      request: { rows: number; columns: number },
      environment?: Record<string, unknown>,
    ): Promise<
      Array<{
        blob: Blob;
        width: number;
        height: number;
        tile: {
          row: number;
          column: number;
          x: number;
          y: number;
          width: number;
          height: number;
        };
      }>
    >;
  };
  export default processors;
}
