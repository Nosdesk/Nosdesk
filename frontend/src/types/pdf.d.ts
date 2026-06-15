declare module 'pdfjs-dist' {
  interface PDFDocumentProxy {
    getPage(pageNumber: number): Promise<PDFPageProxy>;
    numPages: number;
  }

  interface PDFPageProxy {
    getViewport(options: { scale: number }): PDFPageViewport;
    render(options: {
      canvasContext: CanvasRenderingContext2D;
      viewport: PDFPageViewport;
    }): { promise: Promise<void> };
  }

  interface PDFPageViewport {
    width: number;
    height: number;
  }

  interface GlobalWorkerOptions {
    workerSrc: string;
  }

  interface GetDocumentParams {
    url?: string;
    data?: Uint8Array;
    /** When false, pdf.js never uses `new Function()` (falls back to its
     * interpreter), keeping it within a strict `script-src 'self'` CSP. */
    isEvalSupported?: boolean;
  }

  interface PDFDocumentLoadingTask {
    promise: Promise<PDFDocumentProxy>;
  }

  export const GlobalWorkerOptions: GlobalWorkerOptions;
  
  export function getDocument(src: string | GetDocumentParams): PDFDocumentLoadingTask;
} 