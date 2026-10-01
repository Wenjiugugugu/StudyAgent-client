/** A rounded lens in CSS pixels; the centre is neutral, only the bevel refracts. */
export function liquidGlassVector(
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
) {
  const r = Math.max(1, Math.min(radius, width / 2, height / 2));
  const px = x - width / 2;
  const py = y - height / 2;
  const qx = Math.abs(px) - width / 2 + r;
  const qy = Math.abs(py) - height / 2 + r;
  const ox = Math.max(qx, 0);
  const oy = Math.max(qy, 0);
  const distance = Math.hypot(ox, oy) + Math.min(Math.max(qx, qy), 0) - r;
  const rim = Math.min(18, height * 0.22, width * 0.22);
  if (distance >= 0 || distance <= -rim) return [0, 0] as const;

  // Convex bevel, strongest just inside the silhouette. Normals follow the
  // actual corner radius, never a stretched oval shared by different shapes.
  const t = -distance / rim;
  const bend = Math.pow(Math.sin(Math.PI * t), 0.8);
  const length = Math.hypot(ox, oy);
  const nx = length ? ox / length : qx > qy ? 1 : 0;
  const ny = length ? oy / length : qx > qy ? 0 : 1;
  return [-Math.sign(px) * nx * bend, -Math.sign(py) * ny * bend] as const;
}

export function createLiquidGlassMap(width: number, height: number, radius: number): string {
  const canvas = document.createElement("canvas");
  // Bound texture work on large panels; coordinates stay in CSS pixels.
  const resolution = Math.min(1, 640 / Math.max(width, height));
  canvas.width = Math.max(1, Math.round(width * resolution));
  canvas.height = Math.max(1, Math.round(height * resolution));
  const context = canvas.getContext("2d");
  if (!context) return "";
  const image = context.createImageData(canvas.width, canvas.height);
  for (let y = 0; y < canvas.height; y++) {
    for (let x = 0; x < canvas.width; x++) {
      const [dx, dy] = liquidGlassVector(
        ((x + 0.5) / canvas.width) * width,
        ((y + 0.5) / canvas.height) * height,
        width,
        height,
        radius
      );
      const i = (y * canvas.width + x) * 4;
      image.data[i] = Math.round(128 + dx * 127);
      image.data[i + 1] = Math.round(128 + dy * 127);
      image.data[i + 2] = 128;
      image.data[i + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
  return canvas.toDataURL("image/png");
}
