/**
 * The canvas painter: pages, covers, endpapers. See layout/types.ts for the
 * contract (`PaintApi`).
 */
export { paintPage, motionAt } from "./page";
export { paintSurface } from "./surface";
export { drawOrnament } from "./ornaments";
export { audioWidgetLayout, galleryWidgetLayout, rowWidgetLayout, videoWidgetLayout } from "./widgets";
export { createCanvasMeasurer } from "./measure";
export type { Rect } from "../layout/widgetMetrics";
