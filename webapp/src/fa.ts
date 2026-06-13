/** All user-facing Persian copy, verbatim from the design handoff
 *  (.context/templates — informal تو register, final). Latin strings that sit
 *  in LTR captions live here too so every word ships from one place. */

export const fa = {
  header: {
    wordmarkA: 'Visual',
    wordmarkB: 'Ears',
    domain: 'visualears.com',
    modelReady: 'مدل آماده‌ست',
    modelLoading: 'در حال بارگذاری مدل',
  },

  loader: {
    title: 'چیزی که نمیشنوی رو ببین',
    lead: 'رونویسی زندهٔ فارسی، کاملاً روی دستگاه خودت. نه اینترنت، نه حساب کاربری.',
    whichModel: 'کدوم مدل؟',
    recommended: 'پیشنهادی',
    capSimd: 'WebAssembly SIMD',
    capThreads: (n: string) => `چند‌رشته‌ای (${n} رشته)`,
    capWebGpu: 'WebGPU',
    capWebGpuNotNeeded: 'لازم نیست — CPU کافیه',
    capWebGpuActive: 'فعاله — سریع‌تر هم شد',
    download: 'دانلود و بارگذاری مدل',
    downloading: 'در حال دانلود…',
    initWasm: 'در حال راه‌اندازی WASM…',
    initWebGpu: 'در حال راه‌اندازی WebGPU…',
    ready: 'آماده!',
    cachedCaption: (file: string) => `${file} — once cached, loads instantly offline`,
    footer: 'هیچ داده‌ای از دستگاهت خارج نمی‌شه · مدل فقط یک‌بار دانلود می‌شه',
    loadFailed: 'بارگذاری مدل به مشکل خورد — یه بار دیگه امتحان کن.',
    retry: 'تلاش دوباره',
  },

  mode: {
    title: 'چی‌کار کنیم؟',
    live: {
      title: 'زیرنویس زنده',
      body: 'صدای میکروفون یا سیستم رو همین حالا رونویسی کن — با تفکیک گوینده.',
    },
    media: {
      title: 'زیرنویس رسانه',
      body: 'فایل ویدیو یا صوتی رو بنداز اینجا؛ زیرنویس و متن زمان‌دار تحویل بگیر.',
    },
  },

  live: {
    title: 'زیرنویس زنده',
    back: 'بازگشت',
    sourceMic: 'میکروفون',
    sourceSystem: 'صدای سیستم',
    pause: 'توقف',
    resume: 'ادامه',
    speaker: (n: string) => `گوینده ${n}`,
    quietEmpty: 'الان ساکته. هر وقت صدایی بیاد، همین‌جا می‌بینیش.',
    noSignal: 'صدایی به گوشم نمی‌رسه — میکروفون یا منبع صدا رو چک کن.',
    micDenied: 'دسترسی به میکروفون رد شد — از تنظیمات مرورگر اجازه بده.',
    noSystemAudio: 'این پنجره صدا نداره — یه تب یا پنجرهٔ باصدا انتخاب کن.',
    systemUnsupported: 'این مرورگر صدای سیستم رو نمی‌ده — از Chrome یا Edge استفاده کن.',
  },

  media: {
    title: 'زیرنویس رسانه',
    back: 'بازگشت',
    dropTitle: 'فایل رو بنداز اینجا',
    formats: 'mp4 · mov · mkv · mp3 · wav · m4a',
    browse: 'انتخاب فایل',
    working: 'در حال رونویسی…',
    srt: 'SRT',
    vtt: 'VTT',
    decodeFailed: 'این فایل رو نتونستم بخونم — یه فرمت دیگه امتحان کن.',
    nothingHeard: 'صدایی توی این فایل پیدا نکردم.',
  },
} as const;
