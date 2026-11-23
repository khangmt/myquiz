import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ApiFailure } from "../api/client";
import type { MediaDoc } from "../generated/MediaDoc";

/** Upload or remove an image. The alt text is asked for before upload. */
export function ImageField({
  url,
  label,
  onChange,
}: {
  url: string | null;
  label: string;
  onChange: (media: MediaDoc | null) => void;
}) {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  const [alt, setAlt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function upload(file: File) {
    if (file.size > 5 * 1024 * 1024) {
      setError("FILE_TOO_LARGE_IMAGE");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const form = new FormData();
      form.append("file", file);
      if (alt.trim()) form.append("alt", alt.trim());
      onChange(await api<MediaDoc>("/media", { method: "POST", form }));
      setAlt("");
    } catch (e) {
      setError(e instanceof ApiFailure ? (e.code === "FILE_TOO_LARGE" ? "FILE_TOO_LARGE_IMAGE" : e.code) : "INTERNAL");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      {url ? (
        <>
          <img src={url} alt="" className="h-16 w-auto rounded border border-slate-200 object-contain" />
          <button type="button" className="text-red-700 underline" onClick={() => onChange(null)}>
            {t("editor.removeImage")}
          </button>
        </>
      ) : (
        <>
          <input
            value={alt}
            maxLength={300}
            onChange={(e) => setAlt(e.target.value)}
            placeholder={t("editor.altPlaceholder")}
            aria-label={t("editor.altPlaceholder")}
            className="min-w-0 flex-1 rounded border border-slate-300 px-2 py-1"
          />
          <button
            type="button"
            disabled={busy}
            onClick={() => input.current?.click()}
            className="rounded border border-slate-300 bg-white px-2 py-1 disabled:opacity-50"
          >
            {busy ? t("editor.uploading") : label}
          </button>
          <input
            ref={input}
            type="file"
            accept="image/jpeg,image/png,image/webp"
            className="hidden"
            data-testid="image-input"
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (f) void upload(f);
            }}
          />
        </>
      )}
      {error && (
        <span role="alert" className="w-full text-red-700">
          {t(`errors.${error}`, { defaultValue: t(`problems.${error}`, { defaultValue: t("errors.INTERNAL") }) })}
        </span>
      )}
    </div>
  );
}
