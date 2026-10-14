import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowDownToLine,
  ArrowRight,
  Check,
  ChevronRight,
  CircleHelp,
  FolderOpen,
  Image as ImageIcon,
  Images,
  Languages,
  LoaderCircle,
  Moon,
  Plus,
  RefreshCw,
  ScanLine,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import { bytes, cancelTask, command, native } from "./bridge";
import {
  defaults,
  type Format,
  type Imported,
  type Job,
  type Options,
  type Preset,
  type Report,
  type ResizeMode,
  type Strategy,
} from "./types";
import Preview from "./Preview";
import { processingNote } from "./notes";

function saved<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(`squeeze-pic-${key}`);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}

export default function App() {
  const [en, setEn] = useState(() => saved("english", false));
  const [dark, setDark] = useState(() => saved("dark", false));
  const [options, setOptions] = useState<Options>(() => ({
    ...defaults,
    ...saved<Partial<Options>>("options", {}),
    resize: {
      ...defaults.resize,
      ...saved<Partial<Options>>("options", {}).resize,
    },
  }));
  const [mode, setMode] = useState<"compress" | "convert">("compress");
  const [convertFormat, setConvertFormat] = useState<Format>("png");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [directory, setDirectory] = useState(() => saved("directory", ""));
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [importing, setImporting] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [message, setMessage] = useState("");
  const [showHelp, setShowHelp] = useState(false);
  const busyRef = useRef(false);
  const cancelRef = useRef(false);
  const jobsRef = useRef(jobs);
  const helpRef = useRef<HTMLDialogElement>(null);
  const previewRef = useRef<HTMLDialogElement>(null);
  const t = (zh: string, english: string) => (en ? english : zh);
  useEffect(() => {
    jobsRef.current = jobs;
  }, [jobs]);
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    localStorage.setItem("squeeze-pic-dark", JSON.stringify(dark));
  }, [dark]);
  useEffect(() => {
    document.documentElement.lang = en ? "en" : "zh-CN";
    localStorage.setItem("squeeze-pic-english", JSON.stringify(en));
  }, [en]);
  useEffect(() => {
    localStorage.setItem("squeeze-pic-options", JSON.stringify(options));
  }, [options]);
  useEffect(() => {
    localStorage.setItem("squeeze-pic-directory", JSON.stringify(directory));
  }, [directory]);
  useEffect(() => {
    if (showHelp) helpRef.current?.showModal();
    else helpRef.current?.close();
  }, [showHelp]);
  useEffect(() => {
    if (previewOpen) previewRef.current?.showModal();
    else previewRef.current?.close();
  }, [previewOpen, dark]);
  const patch = (path: string, patch: Partial<Job>) =>
    setJobs((items) =>
      items.map((j) => (j.path === path ? { ...j, ...patch } : j)),
    );
  const add = async (paths: string[]) => {
    if (!paths.length || busyRef.current) return;
    busyRef.current = true;
    setImporting(true);
    setMessage("");
    try {
      const existing = new Set(jobsRef.current.map((j) => j.path));
      const imported = await command<Imported[]>("import_files", {
        paths: paths.filter((p) => !existing.has(p)).slice(0, 100),
      });
      setJobs((current) => {
        const paths = new Set(current.map((j) => j.path));
        return [
          ...current,
          ...imported
            .filter((j) => !paths.has(j.path))
            .map((j) => ({
              ...j,
              status: j.error ? ("unsupported" as const) : ("queued" as const),
            })),
        ];
      });
      setSelected(
        (current) =>
          current ||
          imported.find((j) => j.info)?.path ||
          imported[0]?.path ||
          null,
      );
    } catch (e) {
      setMessage(String(e));
    } finally {
      busyRef.current = false;
      setImporting(false);
      setDragging(false);
    }
  };
  const addRef = useRef(add);
  addRef.current = add;
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    const cleanups: (() => void)[] = [];
    getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter")
          setDragging(true);
        if (event.payload.type === "leave") setDragging(false);
        if (event.payload.type === "drop") {
          setDragging(false);
          void addRef.current(event.payload.paths);
        }
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanups.push(unlisten);
      });
    listen<{ path: string; phase: string }>("pic-progress", (event) =>
      patch(event.payload.path, { phase: event.payload.phase }),
    ).then((unlisten) => {
      if (disposed) unlisten();
      else cleanups.push(unlisten);
    });
    return () => {
      disposed = true;
      cleanups.forEach((f) => f());
    };
  }, []);
  const pickFiles = async () => {
    if (!native) {
      setMessage(
        t(
          "请在 Squeeze Pic 桌面应用中导入本地图片。",
          "Open Squeeze Pic desktop to process local images.",
        ),
      );
      return;
    }
    try {
      const paths = await open({
        multiple: true,
        filters: [
          {
            name: t("图片", "Images"),
            extensions: [
              "jpg",
              "jpeg",
              "png",
              "webp",
              "avif",
              "gif",
              "bmp",
              "tif",
              "tiff",
              "ico",
              "icns",
              "svg",
              "heic",
              "heif",
              "jxl",
            ],
          },
        ],
      });
      if (paths) await add(Array.isArray(paths) ? paths : [paths]);
    } catch (e) {
      setMessage(String(e));
    }
  };
  const pickDirectory = async () => {
    if (!native) return;
    try {
      const path = await open({ directory: true, multiple: false });
      if (typeof path === "string") setDirectory(path);
    } catch (e) {
      setMessage(String(e));
    }
  };
  const active = jobs.find((j) => j.path === selected);
  const targetFormat =
    mode === "compress" ? active?.info?.format : convertFormat;
  const losslessFormat = ["png", "bmp", "tiff", "ico", "icns"].includes(
    targetFormat || "",
  );
  const pending = jobs.filter(
    (j) => j.info && ["queued", "failed", "cancelled"].includes(j.status),
  );
  const originalBytes = jobs.reduce((n, j) => n + (j.info?.bytes || 0), 0);
  const savedBytes = jobs.reduce(
    (n, j) =>
      n + (j.report ? j.report.original_bytes - j.report.result_bytes : 0),
    0,
  );
  const complete = jobs.filter((j) => j.report).length;
  const strictResize =
    options.preset === "lossless" && options.resize.mode !== "original";
  const strictJpeg =
    options.preset === "lossless" &&
    mode === "convert" &&
    convertFormat === "jpeg" &&
    pending.some((j) => j.info?.format !== "jpeg");
  const invalidSize =
    options.resize.mode === "fit"
      ? options.resize.width < 1 ||
        options.resize.width > 100000 ||
        options.resize.height < 1 ||
        options.resize.height > 100000
      : options.resize.mode !== "original" &&
        (options.resize.value < 1 ||
          options.resize.value >
            (options.resize.mode === "percent" ? 400 : 100000));
  const conflict = strictResize
    ? t(
        "严格无损需要保留原始尺寸。",
        "Strict lossless requires original dimensions.",
      )
    : strictJpeg
      ? t(
          "严格无损 JPEG 仅能优化已有 JPEG。",
          "Lossless JPEG requires JPEG inputs.",
        )
      : invalidSize
        ? t("请输入有效的输出尺寸。", "Enter a valid output size.")
        : "";
  const start = async () => {
    if (!native || busyRef.current || conflict || !pending.length) return;
    busyRef.current = true;
    cancelRef.current = false;
    setBusy(true);
    setMessage("");
    const batch = pending.map((j) => j.path);
    const snapshot = structuredClone({
      ...options,
      format: mode === "compress" ? null : convertFormat,
    });
    try {
      for (const path of batch) {
        if (cancelRef.current) break;
        setSelected(path);
        patch(path, {
          status: "processing",
          error: null,
          report: undefined,
          phase: "reading",
        });
        try {
          const report = await command<Report>("process_image", {
            path,
            directory: directory || null,
            options: snapshot,
          });
          patch(path, { status: report.outcome, report, phase: undefined });
        } catch (e) {
          patch(path, {
            status: cancelRef.current ? "cancelled" : "failed",
            error: cancelRef.current ? null : String(e),
            phase: undefined,
          });
        }
      }
      if (cancelRef.current)
        setJobs((current) =>
          current.map((j) =>
            batch.includes(j.path) && j.status === "queued"
              ? { ...j, status: "cancelled" }
              : j,
          ),
        );
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const cancel = async () => {
    cancelRef.current = true;
    try {
      await cancelTask();
    } catch (e) {
      setMessage(String(e));
    }
  };
  const update = <K extends keyof Options>(key: K, value: Options[K]) =>
    setOptions((o) => ({ ...o, [key]: value }));
  const resize = (patch: Partial<Options["resize"]>) =>
    setOptions((o) => ({ ...o, resize: { ...o.resize, ...patch } }));
  const choosePreset = (preset: Preset) =>
    setOptions((o) => ({
      ...o,
      preset,
      resize:
        preset === "lossless" ? { ...o.resize, mode: "original" } : o.resize,
    }));
  const setResizeMode = (mode: ResizeMode) =>
    resize({ mode, value: mode === "percent" ? 50 : 1920 });
  const statuses: Record<Job["status"], string> = {
    queued: t("等待处理", "Queued"),
    processing: t("处理中", "Processing"),
    saved: t("已导出", "Exported"),
    unchanged: t("无需优化", "Already optimized"),
    failed: t("处理失败", "Failed"),
    cancelled: t("已取消", "Cancelled"),
    unsupported: t("暂不支持", "Unsupported"),
  };
  const phase = (value?: string) =>
    ({
      reading: t("读取图片", "Reading"),
      resizing: t("调整尺寸", "Resizing"),
      encoding: t("压缩编码", "Encoding"),
      validating: t("校验结果", "Validating"),
      saving: t("保存文件", "Saving"),
      completed: t("处理完成", "Completed"),
    })[value || ""] || t("处理中", "Processing");
  const formats: [Format, string][] = [
    ["png", "PNG"],
    ["jpeg", "JPEG"],
    ["webp", "WebP"],
    ["avif", "AVIF"],
    ["jxl", "JPEG XL"],
    ["heif", "HEIF"],
    ["gif", "GIF"],
    ["tiff", "TIFF"],
    ["bmp", "BMP"],
    ["ico", "ICO"],
    ["icns", "ICNS"],
  ];
  const switchMode = (next: "compress" | "convert") => {
    setMode(next);
    setMessage("");
    setPreviewOpen(false);
    if (next === "convert" && options.preset === "lossless")
      choosePreset("balanced");
    setJobs((current) =>
      current.map((j) =>
        j.info ? { ...j, status: "queued", report: undefined, error: null } : j,
      ),
    );
  };
  const action =
    mode === "compress"
      ? t("压缩图片", "Compress images")
      : t("转换格式", "Convert images");
  return (
    <div className="app-shell simple-app" key={dark ? "dark" : "light"}>
      <header className="topbar">
        <div className="brand">
          <img src="/logo.svg" width="32" height="32" alt="" />
          <h1>
            Squeeze <span>Pic</span>
          </h1>
        </div>
        <nav className="tool-tabs" aria-label={t("工具", "Tools")}>
          <button
            disabled={busy || importing}
            aria-pressed={mode === "compress"}
            className={mode === "compress" ? "active" : ""}
            onClick={() => switchMode("compress")}
          >
            {t("压缩图片", "Compress")}
          </button>
          <button
            disabled={busy || importing}
            aria-pressed={mode === "convert"}
            className={mode === "convert" ? "active" : ""}
            onClick={() => switchMode("convert")}
          >
            {t("格式转换", "Convert")}
          </button>
        </nav>
        <div className="header-actions">
          <button
            className="icon-button"
            aria-label={t("切换英文", "Switch language")}
            onClick={() => setEn(!en)}
          >
            <Languages size={18} />
          </button>
          <button
            className="icon-button"
            aria-label={t("切换主题", "Toggle theme")}
            onClick={() => setDark(!dark)}
          >
            {dark ? <Sun size={18} /> : <Moon size={18} />}
          </button>
          <button
            className="icon-button"
            aria-label={t("使用说明", "Help")}
            onClick={() => setShowHelp(true)}
          >
            <CircleHelp size={18} />
          </button>
        </div>
      </header>
      {!native && (
        <div className="browser-banner">
          {t(
            "界面预览模式 · 请使用桌面应用处理图片",
            "Preview mode · Processing requires the desktop app",
          )}
        </div>
      )}
      <main className="workspace">
        <aside className="settings-panel" aria-label={t("设置", "Settings")}>
          <div className="settings-title">
            <h2>{t("设置", "Settings")}</h2>
            <button
              className="icon-button"
              disabled={busy || importing}
              aria-label={t("恢复默认设置", "Reset settings")}
              onClick={() => setOptions(structuredClone(defaults))}
            >
              <RefreshCw size={15} />
            </button>
          </div>
          <fieldset className="settings-fields" disabled={busy || importing}>
            {mode === "convert" && (
              <div className="setting-group">
                <h3>{t("转换为", "Convert to")}</h3>
                <div className="format-grid">
                  {formats.map(([id, title]) => (
                    <button
                      key={id}
                      className={`format-card ${convertFormat === id ? "active" : ""}`}
                      aria-pressed={convertFormat === id}
                      onClick={() => {
                        setConvertFormat(id);
                        if (
                          ["gif", "heif", "avif", "ico", "icns"].includes(id) &&
                          options.preset === "lossless"
                        )
                          choosePreset("balanced");
                      }}
                    >
                      <strong>{title}</strong>
                      {convertFormat === id && <Check size={12} />}
                    </button>
                  ))}
                </div>
                <p className="setting-note">
                  {convertFormat === "heif"
                    ? t(
                        "HEIF 使用 macOS 系统编解码器。",
                        "HEIF uses macOS system codecs.",
                      )
                    : convertFormat === "ico" || convertFormat === "icns"
                      ? t(
                          "按图标规格等比缩放并留透明边。",
                          "Fits the image into standard icon sizes.",
                        )
                      : t(
                          "SVG 可导入并转换为以上图片格式。",
                          "SVG can be imported and converted to these formats.",
                        )}
                </p>
              </div>
            )}
            {(mode === "compress" || !losslessFormat) && (
              <div className="setting-group">
                <label className="simple-label" htmlFor="quality">
                  {t("画质", "Quality")}
                </label>
                <select
                  id="quality"
                  value={options.preset}
                  onChange={(e) => choosePreset(e.target.value as Preset)}
                >
                  <option value="high">{t("高画质", "High quality")}</option>
                  <option value="balanced">
                    {t("均衡（推荐）", "Balanced (recommended)")}
                  </option>
                  <option value="small">{t("更小体积", "Smaller file")}</option>
                  <option value="custom">{t("自定义", "Custom")}</option>
                  {!["gif", "heif", "avif", "svg"].includes(
                    targetFormat || "",
                  ) && (
                    <option value="lossless">
                      {t("严格无损", "Strict lossless")}
                    </option>
                  )}
                </select>
                {options.preset === "custom" && (
                  <div className="quality-slider">
                    <input
                      type="range"
                      min="1"
                      max="100"
                      value={options.quality}
                      aria-label={t("自定义画质", "Custom quality")}
                      onChange={(e) =>
                        update("quality", Number(e.target.value))
                      }
                    />
                    <output>{options.quality}</output>
                  </div>
                )}
              </div>
            )}
            {mode === "compress" && (
              <div className="setting-group">
                <label className="simple-label" htmlFor="strategy">
                  {t("压缩策略", "Compression strategy")}
                </label>
                <select
                  id="strategy"
                  value={options.strategy}
                  onChange={(e) =>
                    update("strategy", e.target.value as Strategy)
                  }
                >
                  <option value="fast">{t("快速", "Fast")}</option>
                  <option value="smart">
                    {t("智能（推荐）", "Smart (recommended)")}
                  </option>
                  <option value="deep">{t("深度压缩", "Deep")}</option>
                </select>
              </div>
            )}
            <div className="setting-group">
              <label className="simple-label" htmlFor="resize-mode">
                {t("输出尺寸", "Output size")}
              </label>
              <select
                id="resize-mode"
                value={options.resize.mode}
                disabled={options.preset === "lossless"}
                onChange={(e) => setResizeMode(e.target.value as ResizeMode)}
              >
                <option value="original">
                  {t("原始尺寸", "Original size")}
                </option>
                <option value="percent">
                  {t("按比例缩放", "Scale by percentage")}
                </option>
                <option value="longest">
                  {t("限制最长边", "Limit longest edge")}
                </option>
                <option value="width">{t("指定宽度", "Set width")}</option>
                <option value="height">{t("指定高度", "Set height")}</option>
                <option value="fit">
                  {t("适应宽高", "Fit inside dimensions")}
                </option>
              </select>
              {options.resize.mode !== "original" &&
                (options.resize.mode === "fit" ? (
                  <div className="size-inputs">
                    <label>
                      {t("宽", "Width")}
                      <input
                        type="number"
                        min="1"
                        max="100000"
                        value={options.resize.width || ""}
                        onChange={(e) =>
                          resize({ width: Number(e.target.value) })
                        }
                      />
                    </label>
                    <span>×</span>
                    <label>
                      {t("高", "Height")}
                      <input
                        type="number"
                        min="1"
                        max="100000"
                        value={options.resize.height || ""}
                        onChange={(e) =>
                          resize({ height: Number(e.target.value) })
                        }
                      />
                    </label>
                  </div>
                ) : (
                  <div className="single-size">
                    <input
                      type="number"
                      min="1"
                      max={options.resize.mode === "percent" ? 400 : 100000}
                      value={options.resize.value || ""}
                      aria-label={t("输出尺寸数值", "Output size value")}
                      onChange={(e) =>
                        resize({ value: Number(e.target.value) })
                      }
                    />
                    <span>
                      {options.resize.mode === "percent" ? "%" : "px"}
                    </span>
                  </div>
                ))}
            </div>
            <details className="advanced">
              <summary>{t("更多选项", "More options")}</summary>
              <label className="advanced-label">
                {t("拍摄信息", "Camera metadata")}
                <select
                  value={options.metadata}
                  onChange={(e) =>
                    update("metadata", e.target.value as Options["metadata"])
                  }
                >
                  <option value="preserve">{t("保留", "Preserve")}</option>
                  <option value="strip">{t("移除", "Remove")}</option>
                </select>
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={options.resize.allow_upscale}
                  onChange={(e) => resize({ allow_upscale: e.target.checked })}
                />
                {t("允许放大", "Allow upscaling")}
              </label>
            </details>
            <div className="output-folder">
              <h3>{t("保存位置", "Save to")}</h3>
              <button className="folder-button" onClick={pickDirectory}>
                <FolderOpen size={16} />
                <span>
                  {directory || t("原图所在文件夹", "Original folder")}
                </span>
                <ChevronRight size={14} />
              </button>
              {directory && (
                <button
                  className="text-button muted"
                  onClick={() => setDirectory("")}
                >
                  {t("恢复默认位置", "Use original folder")}
                </button>
              )}
            </div>
          </fieldset>
        </aside>
        <div className="work-area">
          <section
            className={`files-panel ${dragging ? "dragging" : ""}`}
            aria-label={t("图片列表", "Image list")}
          >
            <div className="section-bar">
              <div className="section-title">
                <Images size={18} />
                <h2>{t("图片", "Images")}</h2>
                <span className="count">{jobs.length}</span>
              </div>
              <div className="queue-actions">
                {jobs.length > 0 && (
                  <button
                    className="icon-button"
                    disabled={busy || importing}
                    aria-label={t("清空列表", "Clear list")}
                    onClick={() => {
                      setJobs([]);
                      setSelected(null);
                    }}
                  >
                    <Trash2 size={16} />
                  </button>
                )}
                <button
                  className="small-button"
                  disabled={busy || importing}
                  onClick={pickFiles}
                >
                  <Plus size={16} />
                  {t("添加图片", "Add images")}
                </button>
              </div>
            </div>
            {jobs.length === 0 ? (
              <div className="simple-dropzone">
                <ImageIcon size={42} strokeWidth={1.3} />
                <h3>{t("拖入图片到这里", "Drop images here")}</h3>
                <p>
                  {t(
                    "或点击选择，支持批量处理",
                    "Or choose files to process a batch",
                  )}
                </p>
                <button
                  className="primary"
                  disabled={importing}
                  onClick={pickFiles}
                >
                  {t("选择图片", "Choose images")}
                </button>
                <small>
                  PNG · JPEG · WebP · AVIF · JPEG XL · HEIF · GIF · TIFF · BMP ·
                  ICO · ICNS · SVG
                </small>
              </div>
            ) : (
              <ul className="file-list">
                {jobs.map((j) => (
                  <li
                    key={j.path}
                    className={`file-row ${j.path === selected ? "selected" : ""}`}
                  >
                    <button
                      className="file-main"
                      aria-pressed={j.path === selected}
                      onClick={() => setSelected(j.path)}
                    >
                      {j.thumbnail ? (
                        <img src={j.thumbnail} alt="" />
                      ) : (
                        <ImageIcon size={30} />
                      )}
                      <span>
                        <strong>{j.name}</strong>
                        <small>
                          {j.info
                            ? `${j.info.format.toUpperCase()} · ${j.info.display_width} × ${j.info.display_height} · ${bytes(j.info.bytes)}`
                            : j.error}
                        </small>
                      </span>
                    </button>
                    <div className="file-status">
                      {j.report && (
                        <strong className="savings">
                          {bytes(j.report.result_bytes)}
                        </strong>
                      )}
                      <span>
                        {j.status === "processing"
                          ? phase(j.phase)
                          : statuses[j.status]}
                      </span>
                    </div>
                    <button
                      className="icon-button"
                      disabled={busy || importing}
                      aria-label={`${t("移除", "Remove")} ${j.name}`}
                      onClick={() =>
                        setJobs((items) =>
                          items.filter((item) => item.path !== j.path),
                        )
                      }
                    >
                      <X size={16} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
          {active?.info && (
            <button
              className="preview-launcher"
              aria-haspopup="dialog"
              onClick={() => setPreviewOpen(true)}
            >
              <ScanLine size={18} />
              <span>
                <strong>{t("细节对比", "Compare details")}</strong>
                <small>
                  {t("点击打开预览窗口", "Open comparison preview")}
                </small>
              </span>
              <ChevronRight size={17} />
            </button>
          )}
          {active?.error && (
            <p className="inline-error" role="alert">
              {active.error}
            </p>
          )}
          {active?.report?.output && (
            <div className="result-links">
              <Check size={16} />
              {t("已保存", "Saved")}
              <button
                className="text-button"
                onClick={() =>
                  command("open_result", {
                    path: active.report!.output,
                    folder: false,
                  })
                }
              >
                {t("打开图片", "Open image")}
              </button>
              <button
                className="text-button"
                onClick={() =>
                  command("open_result", {
                    path: active.report!.output,
                    folder: true,
                  })
                }
              >
                {t("所在文件夹", "Show folder")}
              </button>
            </div>
          )}
          {!!active?.report?.warnings.length && (
            <details className="result-notes">
              <summary>{t("处理说明", "Processing notes")}</summary>
              <ul>
                {active.report.warnings.map((warning, i) => (
                  <li key={i}>{processingNote(warning, en)}</li>
                ))}
              </ul>
            </details>
          )}
          <p className="privacy-note">
            {t(
              "图片仅在本机处理，原件始终保留。",
              "Processed locally. Your originals are kept.",
            )}
          </p>
        </div>
      </main>
      {(message || conflict) && (
        <div className="global-message" role="alert">
          <span>{message || conflict}</span>
          <button
            className="icon-button"
            aria-label={t("关闭提示", "Dismiss")}
            onClick={() => setMessage("")}
          >
            <X size={14} />
          </button>
        </div>
      )}
      <footer className="bottom-bar">
        <div className="batch-stats">
          <span>
            {jobs.length} {t("张图片", "images")} · {bytes(originalBytes)}
          </span>
          {complete > 0 && (
            <span className="savings">
              {t("已节省", "Saved")} {bytes(Math.max(0, savedBytes))}
            </span>
          )}
        </div>
        <div className="run-actions">
          {complete > 0 && !busy && (
            <button
              className="text-button muted"
              onClick={() =>
                setJobs((items) =>
                  items.map((j) =>
                    j.info
                      ? {
                          ...j,
                          status: "queued",
                          report: undefined,
                          error: null,
                        }
                      : j,
                  ),
                )
              }
            >
              {t("重新处理", "Run again")}
            </button>
          )}
          {busy ? (
            <button className="cancel-button" onClick={cancel}>
              <X size={16} />
              {t("取消", "Cancel")}
            </button>
          ) : (
            <button
              className="primary start-button"
              disabled={!native || !pending.length || importing || !!conflict}
              onClick={start}
            >
              {importing ? (
                <LoaderCircle className="spin" size={17} />
              ) : (
                <ArrowDownToLine size={17} />
              )}
              <span>{importing ? t("导入中…", "Importing…") : action}</span>
              <ArrowRight size={17} />
            </button>
          )}
        </div>
      </footer>
      <dialog
        ref={previewRef}
        className="preview-dialog"
        aria-labelledby="preview-title"
        closedby="any"
        onClose={() => setPreviewOpen(false)}
        onClick={(e) => {
          const dialog = e.currentTarget;
          if ("closedBy" in HTMLDialogElement.prototype || e.target !== dialog)
            return;
          const rect = dialog.getBoundingClientRect();
          if (
            e.clientX < rect.left ||
            e.clientX > rect.right ||
            e.clientY < rect.top ||
            e.clientY > rect.bottom
          )
            dialog.close();
        }}
      >
        {previewOpen && (
          <Preview
            job={active}
            en={en}
            busy={busy || importing}
            onClose={() => previewRef.current?.close()}
          />
        )}
      </dialog>
      <dialog
        ref={helpRef}
        onClose={() => setShowHelp(false)}
        className="help-dialog"
      >
        <form method="dialog">
          <div className="section-bar">
            <h2>Squeeze Pic</h2>
            <button className="icon-button" aria-label={t("关闭", "Close")}>
              <X size={18} />
            </button>
          </div>
          <p>
            {t(
              "压缩图片：保留原格式，按画质、策略与尺寸减小文件。格式转换：选择输出格式，批量生成新图片。",
              "Compress keeps the original format. Convert creates images in your chosen format.",
            )}
          </p>
          <p>
            {t(
              "支持静态图片。GIF 动画、HDR 和 16 位图片暂不支持。SVG 仅支持导入转换；HEIF 在 macOS 上可用。",
              "Static images only. Animated GIF, HDR and 16-bit images are not supported. SVG is import-only; HEIF requires macOS.",
            )}
          </p>
          <p>
            {t(
              "全程离线，不覆盖原件。",
              "Offline processing. Originals are never overwritten.",
            )}
          </p>
          <button className="primary">{t("知道了", "Got it")}</button>
        </form>
      </dialog>
    </div>
  );
}
