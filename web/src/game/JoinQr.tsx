import { useEffect, useState } from "react";
import QRCode from "qrcode";

/** The join link as a QR code, for phones in the room (design section 7). */
export function JoinQr({ url, label }: { url: string; label: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    QRCode.toDataURL(url, { margin: 1, width: 240, errorCorrectionLevel: "M" })
      .then((d) => live && setSrc(d))
      .catch(() => live && setSrc(null));
    return () => {
      live = false;
    };
  }, [url]);
  if (!src) return null;
  return <img src={src} alt={label} width={200} height={200} className="rounded-lg bg-white p-2 shadow" />;
}
