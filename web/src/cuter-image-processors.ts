import processors from 'virtual:cuter-image-processors';

export type GridTile = {
  row: number;
  column: number;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type GridSplitOutput = {
  blob: Blob;
  width: number;
  height: number;
  tile: GridTile;
};

export type ImageProcessResult = {
  blob: Blob;
  width: number;
  height: number;
};

export type CuterImageProcessors = {
  MAX_GRID_AXIS: number;
  MAX_GRID_TILES: number;
  MAX_CHROMA_TOLERANCE: number;
  createGridTiles(
    width: number,
    height: number,
    rows: number,
    columns: number,
  ): GridTile[];
  splitImage(
    blob: Blob,
    request: { rows: number; columns: number },
    environment?: Record<string, unknown>,
  ): Promise<GridSplitOutput[]>;
  expandImage(
    blob: Blob,
    request: {
      left: number;
      top: number;
      right: number;
      bottom: number;
      fill?: string;
      color?: string;
    },
    environment?: Record<string, unknown>,
  ): Promise<ImageProcessResult>;
  eraseRectImage(
    blob: Blob,
    request: { x: number; y: number; width: number; height: number },
    environment?: Record<string, unknown>,
  ): Promise<ImageProcessResult>;
  chromaKeyImage(
    blob: Blob,
    request: { color: string; tolerance: number },
    environment?: Record<string, unknown>,
  ): Promise<ImageProcessResult>;
};

const imageProcessors = processors as CuterImageProcessors;

export default imageProcessors;
