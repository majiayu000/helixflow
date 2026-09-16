(function exposeImageProcessors(root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  if (root) root.CuterImageProcessors = api;
})(typeof window !== "undefined" ? window : globalThis, () => {
  const MAX_DIMENSION = 8192;
  const MAX_PIXELS = 16_777_216;
  const MAX_GRID_AXIS = 10;
  const MAX_GRID_TILES = 100;
  const FIT_MODES = Object.freeze(["stretch", "contain", "cover"]);
  const EXPAND_FILLS = Object.freeze(["transparent", "color", "blur"]);
  const MAX_CHROMA_TOLERANCE = 100;
  const HEX_COLOR = /^#[0-9A-Fa-f]{6}$/;

  function positiveInteger(value, label, maximum = Number.MAX_SAFE_INTEGER) {
    if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
      throw new RangeError(`${label}必须是 1 到 ${maximum} 的整数`);
    }
    return value;
  }

  function validateSourceDimensions(width, height) {
    return {
      width: positiveInteger(width, "源图宽度"),
      height: positiveInteger(height, "源图高度"),
    };
  }

  function validateTargetDimensions(width, height) {
    const result = {
      width: positiveInteger(width, "目标宽度", MAX_DIMENSION),
      height: positiveInteger(height, "目标高度", MAX_DIMENSION),
    };
    if (result.width * result.height > MAX_PIXELS) {
      throw new RangeError(`目标画布不能超过 ${MAX_PIXELS.toLocaleString("en-US")} 像素`);
    }
    return result;
  }

  function createResizePlan(sourceWidth, sourceHeight, width, height, fit = "contain") {
    const source = validateSourceDimensions(sourceWidth, sourceHeight);
    const target = validateTargetDimensions(width, height);
    if (!FIT_MODES.includes(fit)) throw new TypeError(`不支持的缩放模式：${fit}`);

    const fullSource = { x: 0, y: 0, width: source.width, height: source.height };
    const fullTarget = { x: 0, y: 0, width: target.width, height: target.height };
    if (fit === "stretch") return { source: fullSource, destination: fullTarget, canvas: target, fit };

    if (fit === "contain") {
      const scale = Math.min(target.width / source.width, target.height / source.height);
      const drawWidth = Math.max(1, Math.round(source.width * scale));
      const drawHeight = Math.max(1, Math.round(source.height * scale));
      return {
        source: fullSource,
        destination: {
          x: Math.floor((target.width - drawWidth) / 2),
          y: Math.floor((target.height - drawHeight) / 2),
          width: drawWidth,
          height: drawHeight,
        },
        canvas: target,
        fit,
      };
    }

    const scale = Math.max(target.width / source.width, target.height / source.height);
    const cropWidth = target.width / scale;
    const cropHeight = target.height / scale;
    return {
      source: {
        x: (source.width - cropWidth) / 2,
        y: (source.height - cropHeight) / 2,
        width: cropWidth,
        height: cropHeight,
      },
      destination: fullTarget,
      canvas: target,
      fit,
    };
  }

  function createGridTiles(width, height, rows, columns) {
    const source = validateSourceDimensions(width, height);
    positiveInteger(rows, "行数", MAX_GRID_AXIS);
    positiveInteger(columns, "列数", MAX_GRID_AXIS);
    const count = rows * columns;
    if (count < 2 || count > MAX_GRID_TILES) {
      throw new RangeError(`宫格切片数量必须是 2 到 ${MAX_GRID_TILES}`);
    }
    const tiles = [];
    for (let row = 0; row < rows; row += 1) {
      const y = Math.floor((row * source.height) / rows);
      const bottom = Math.floor(((row + 1) * source.height) / rows);
      for (let column = 0; column < columns; column += 1) {
        const x = Math.floor((column * source.width) / columns);
        const right = Math.floor(((column + 1) * source.width) / columns);
        tiles.push({
          row,
          column,
          x,
          y,
          width: right - x,
          height: bottom - y,
        });
      }
    }
    if (tiles.some((tile) => tile.width < 1 || tile.height < 1)) {
      throw new RangeError("行列数量不能超过源图像素尺寸");
    }
    return tiles;
  }

  function nonNegativeInteger(value, label, maximum = MAX_DIMENSION) {
    if (!Number.isSafeInteger(value) || value < 0 || value > maximum) {
      throw new RangeError(`${label}必须是 0 到 ${maximum} 的整数`);
    }
    return value;
  }

  function createExpandPlan(width, height, padding = {}) {
    const source = validateSourceDimensions(width, height);
    const left = nonNegativeInteger(padding.left ?? 0, "左扩边");
    const top = nonNegativeInteger(padding.top ?? 0, "上扩边");
    const right = nonNegativeInteger(padding.right ?? 0, "右扩边");
    const bottom = nonNegativeInteger(padding.bottom ?? 0, "下扩边");
    if (left + top + right + bottom < 1) throw new RangeError("至少需要一边扩边大于 0");
    const canvas = validateTargetDimensions(source.width + left + right, source.height + top + bottom);
    return {
      source: { x: 0, y: 0, width: source.width, height: source.height },
      destination: { x: left, y: top, width: source.width, height: source.height },
      canvas,
      padding: { left, top, right, bottom },
    };
  }

  function parseHexColor(value) {
    if (typeof value !== "string" || !HEX_COLOR.test(value)) {
      throw new TypeError("颜色必须是 #RRGGBB");
    }
    return {
      r: Number.parseInt(value.slice(1, 3), 16),
      g: Number.parseInt(value.slice(3, 5), 16),
      b: Number.parseInt(value.slice(5, 7), 16),
    };
  }

  function chromaKeyAlpha(red, green, blue, key, tolerance) {
    const maxDistance = Math.sqrt(3 * 255 * 255);
    const threshold = (tolerance / MAX_CHROMA_TOLERANCE) * maxDistance;
    const softness = maxDistance * 0.08;
    const distance = Math.sqrt((red - key.r) ** 2 + (green - key.g) ** 2 + (blue - key.b) ** 2);
    if (distance <= threshold) return 0;
    if (distance >= threshold + softness) return 255;
    return Math.round(((distance - threshold) / softness) * 255);
  }

  function applyChromaKey(imageData, key, tolerance) {
    if (!imageData?.data || !Number.isSafeInteger(imageData.width) || !Number.isSafeInteger(imageData.height)) {
      throw new TypeError("色度抠图像素数据无效");
    }
    const keyColor = key && Number.isInteger(key.r) ? key : parseHexColor(key);
    const amount = nonNegativeInteger(tolerance, "容差", MAX_CHROMA_TOLERANCE);
    const pixels = imageData.data;
    for (let index = 0; index < pixels.length; index += 4) {
      const alpha = chromaKeyAlpha(pixels[index], pixels[index + 1], pixels[index + 2], keyColor, amount);
      pixels[index + 3] = Math.min(pixels[index + 3], alpha);
    }
    return imageData;
  }

  async function defaultDecode(blob) {
    if (!(blob instanceof Blob)) throw new TypeError("图片处理输入必须是 Blob");
    if (typeof createImageBitmap === "function") return createImageBitmap(blob);
    if (typeof document === "undefined") throw new Error("当前环境不支持图片解码");
    const url = URL.createObjectURL(blob);
    try {
      const image = new Image();
      await new Promise((resolve, reject) => {
        image.addEventListener("load", resolve, { once: true });
        image.addEventListener("error", () => reject(new Error("浏览器无法解码源图片")), { once: true });
        image.src = url;
      });
      return image;
    } finally {
      URL.revokeObjectURL(url);
    }
  }

  function defaultCreateCanvas(width, height) {
    if (typeof OffscreenCanvas === "function") return new OffscreenCanvas(width, height);
    if (typeof document === "undefined") throw new Error("当前环境不支持 Canvas");
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    return canvas;
  }

  async function encodePng(canvas) {
    if (typeof canvas.convertToBlob === "function") {
      const blob = await canvas.convertToBlob({ type: "image/png" });
      if (!blob) throw new Error("Canvas 未生成图片数据");
      return blob;
    }
    if (typeof canvas.toBlob !== "function") throw new Error("当前 Canvas 不支持 PNG 编码");
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("Canvas 未生成图片数据");
    return blob;
  }

  function imageDimensions(image) {
    return validateSourceDimensions(image.width || image.naturalWidth, image.height || image.naturalHeight);
  }

  function drawingContext(canvas, options) {
    const context = options ? canvas.getContext("2d", options) : canvas.getContext("2d");
    if (!context) throw new Error("无法创建 2D Canvas 上下文");
    return context;
  }

  function draw(canvas, image, source, destination) {
    drawingContext(canvas).drawImage(
      image,
      source.x,
      source.y,
      source.width,
      source.height,
      destination.x,
      destination.y,
      destination.width,
      destination.height,
    );
  }

  async function withDecodedImage(blob, environment, operation) {
    const decode = environment.decodeImage || defaultDecode;
    const image = await decode(blob);
    try {
      return await operation(image);
    } finally {
      if (typeof image.close === "function") image.close();
    }
  }

  async function resizeImage(blob, request, environment = {}) {
    return withDecodedImage(blob, environment, async (image) => {
      const source = imageDimensions(image);
      const plan = createResizePlan(source.width, source.height, request.width, request.height, request.fit);
      const canvas = (environment.createCanvas || defaultCreateCanvas)(plan.canvas.width, plan.canvas.height);
      draw(canvas, image, plan.source, plan.destination);
      const output = await (environment.encodeCanvas || encodePng)(canvas);
      if (!(output instanceof Blob)) throw new TypeError("图片编码器必须返回 Blob");
      return { blob: output, width: plan.canvas.width, height: plan.canvas.height, plan };
    });
  }

  async function splitImage(blob, request, environment = {}) {
    return withDecodedImage(blob, environment, async (image) => {
      const source = imageDimensions(image);
      const tiles = createGridTiles(source.width, source.height, request.rows, request.columns);
      const outputs = [];
      for (const tile of tiles) {
        const canvas = (environment.createCanvas || defaultCreateCanvas)(tile.width, tile.height);
        draw(canvas, image, tile, { x: 0, y: 0, width: tile.width, height: tile.height });
        const output = await (environment.encodeCanvas || encodePng)(canvas);
        if (!(output instanceof Blob)) throw new TypeError("图片编码器必须返回 Blob");
        outputs.push({ blob: output, width: tile.width, height: tile.height, tile });
      }
      return outputs;
    });
  }

  async function encodeResult(canvas, environment, width, height, extra) {
    const output = await (environment.encodeCanvas || encodePng)(canvas);
    if (!(output instanceof Blob)) throw new TypeError("图片编码器必须返回 Blob");
    return { blob: output, width, height, ...extra };
  }

  async function expandImage(blob, request, environment = {}) {
    return withDecodedImage(blob, environment, async (image) => {
      const source = imageDimensions(image);
      const plan = createExpandPlan(source.width, source.height, request);
      const fill = request.fill || "transparent";
      if (!EXPAND_FILLS.includes(fill)) throw new TypeError(`不支持的扩边填充：${fill}`);
      const canvas = (environment.createCanvas || defaultCreateCanvas)(plan.canvas.width, plan.canvas.height);
      const context = drawingContext(canvas);
      if (fill === "color") {
        const color = parseHexColor(request.color);
        context.fillStyle = `rgb(${color.r}, ${color.g}, ${color.b})`;
        context.fillRect(0, 0, plan.canvas.width, plan.canvas.height);
      } else if (fill === "blur") {
        if (typeof context.filter !== "string") throw new Error("当前环境不支持模糊扩边");
        context.filter = "blur(24px)";
        context.drawImage(image, 0, 0, plan.canvas.width, plan.canvas.height);
        context.filter = "none";
      }
      context.drawImage(
        image,
        plan.source.x,
        plan.source.y,
        plan.source.width,
        plan.source.height,
        plan.destination.x,
        plan.destination.y,
        plan.destination.width,
        plan.destination.height,
      );
      return encodeResult(canvas, environment, plan.canvas.width, plan.canvas.height, { plan, fill });
    });
  }

  async function chromaKeyImage(blob, request, environment = {}) {
    return withDecodedImage(blob, environment, async (image) => {
      const source = imageDimensions(image);
      const key = parseHexColor(request.color);
      const canvas = (environment.createCanvas || defaultCreateCanvas)(source.width, source.height);
      const context = drawingContext(canvas, { willReadFrequently: true });
      context.drawImage(image, 0, 0);
      const pixels = context.getImageData(0, 0, source.width, source.height);
      applyChromaKey(pixels, key, request.tolerance);
      context.putImageData(pixels, 0, 0);
      return encodeResult(canvas, environment, source.width, source.height, { key, tolerance: request.tolerance });
    });
  }

  return Object.freeze({
    MAX_DIMENSION,
    MAX_PIXELS,
    MAX_GRID_AXIS,
    MAX_GRID_TILES,
    MAX_CHROMA_TOLERANCE,
    FIT_MODES,
    EXPAND_FILLS,
    validateSourceDimensions,
    validateTargetDimensions,
    createResizePlan,
    createGridTiles,
    createExpandPlan,
    parseHexColor,
    chromaKeyAlpha,
    applyChromaKey,
    resizeImage,
    splitImage,
    expandImage,
    chromaKeyImage,
  });
});
