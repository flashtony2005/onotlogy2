/** 手绘/扁平插画风山水场景：浅蓝天空 → 青绿远山 → 深绿松树 + 云朵飞鸟。
 *  纯 SVG 内联——无外部资源，治愈系柔和渐变。 */
export function LoginScene() {
  return (
    <svg
      className="scene-svg"
      viewBox="0 0 900 420"
      preserveAspectRatio="xMidYMax slice"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#aee0f5" />
          <stop offset="60%" stopColor="#e3f6fd" />
          <stop offset="100%" stopColor="#f7fcf0" />
        </linearGradient>
        <linearGradient id="mountain-far" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#b7d9b0" />
          <stop offset="100%" stopColor="#93c49c" />
        </linearGradient>
        <linearGradient id="mountain-near" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#7fbc8b" />
          <stop offset="100%" stopColor="#5da172" />
        </linearGradient>
        <linearGradient id="hill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#6aa77c" />
          <stop offset="100%" stopColor="#4f8f66" />
        </linearGradient>
        <radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">
          <stop offset="0%" stopColor="#fff3c4" />
          <stop offset="100%" stopColor="#ffdf9e" />
        </radialGradient>
      </defs>

      {/* 天空 */}
      <rect width="900" height="420" fill="url(#sky)" />

      {/* 太阳 */}
      <circle cx="720" cy="86" r="34" fill="url(#sun)" opacity="0.95" />

      {/* 云朵（三组椭圆） */}
      <g fill="#ffffff" opacity="0.9">
        <ellipse cx="180" cy="72" rx="52" ry="18" />
        <ellipse cx="150" cy="60" rx="30" ry="14" />
        <ellipse cx="212" cy="62" rx="26" ry="12" />
        <ellipse cx="560" cy="120" rx="44" ry="15" />
        <ellipse cx="532" cy="110" rx="26" ry="11" />
        <ellipse cx="590" cy="112" rx="22" ry="10" />
      </g>

      {/* 飞鸟（三只） */}
      <g stroke="#6b8f9e" strokeWidth="2.6" fill="none" strokeLinecap="round">
        <path d="M330 96 q9 -9 18 0 q9 -9 18 0" />
        <path d="M382 76 q7 -7 14 0 q7 -7 14 0" />
        <path d="M292 70 q6 -6 12 0 q6 -6 12 0" />
      </g>

      {/* 远山（两层青绿） */}
      <path
        d="M0 260 L150 150 L260 220 L400 130 L560 235 L700 165 L900 260 L900 420 L0 420 Z"
        fill="url(#mountain-far)"
        opacity="0.85"
      />
      <path
        d="M0 300 L180 205 L330 275 L520 190 L680 270 L820 225 L900 270 L900 420 L0 420 Z"
        fill="url(#mountain-near)"
        opacity="0.9"
      />

      {/* 近处丘陵 */}
      <path d="M0 340 Q150 295 320 340 T680 335 T900 345 L900 420 L0 420 Z" fill="url(#hill)" />

      {/* 松树（三棵，深绿） */}
      <g>
        {/* 左松树 */}
        <g transform="translate(120 268)">
          <rect x="-3" y="26" width="6" height="14" rx="2" fill="#8a5a3b" />
          <path d="M0 -14 L24 18 L-24 18 Z" fill="#3f7a52" />
          <path d="M0 -28 L20 2 L-20 2 Z" fill="#4c8a5e" />
          <path d="M0 -38 L14 -12 L-14 -12 Z" fill="#5d9c6d" />
        </g>
        {/* 中松树 */}
        <g transform="translate(430 300)">
          <rect x="-4" y="32" width="8" height="16" rx="2" fill="#8a5a3b" />
          <path d="M0 -16 L30 24 L-30 24 Z" fill="#3f7a52" />
          <path d="M0 -34 L24 2 L-24 2 Z" fill="#4c8a5e" />
          <path d="M0 -46 L16 -14 L-16 -14 Z" fill="#5d9c6d" />
        </g>
        {/* 右松树 */}
        <g transform="translate(700 252)">
          <rect x="-3" y="24" width="6" height="12" rx="2" fill="#8a5a3b" />
          <path d="M0 -12 L22 16 L-22 16 Z" fill="#3f7a52" />
          <path d="M0 -25 L18 -2 L-18 -2 Z" fill="#4c8a5e" />
          <path d="M0 -34 L12 -10 L-12 -10 Z" fill="#5d9c6d" />
        </g>
      </g>

      {/* 底部草地 */}
      <rect y="400" width="900" height="20" fill="#e9f7e4" />
    </svg>
  )
}
