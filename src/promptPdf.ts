import {
  getDocument,
  GlobalWorkerOptions,
} from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

export async function extractPromptPdf(
  file: File,
  byteLimit: number,
): Promise<string> {
  const task = getDocument({
    data: new Uint8Array(await file.arrayBuffer()),
    disableFontFace: true,
    useWasm: false,
    enableXfa: false,
  });
  try {
    const pdf = await task.promise;
    if (pdf.numPages > 200) throw new Error("PDF page limit");
    const encoder = new TextEncoder();
    const parts: string[] = [];
    let bytes = 0;
    for (let number = 1; number <= pdf.numPages; number++) {
      const page = await pdf.getPage(number);
      const reader = page.streamTextContent().getReader();
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          for (const item of value.items) {
            if (!("str" in item)) continue;
            const part = item.str + (item.hasEOL ? "\n" : "");
            bytes += encoder.encode(part).length;
            if (bytes > byteLimit) throw new Error("PDF text limit");
            parts.push(part);
          }
        }
      } finally {
        await reader.cancel();
        page.cleanup();
      }
      parts.push("\n\n");
      bytes += 2;
      if (bytes > byteLimit) throw new Error("PDF text limit");
    }
    return parts.join("").trim();
  } finally {
    await task.destroy();
  }
}
