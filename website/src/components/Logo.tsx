import { useId } from "react";

/** The SSHVault mark (a shield around a prompt), same artwork as the app icon. */
export function Logo({ size = 28, className }: { size?: number; className?: string }) {
  const id = useId();
  return (
    <svg width={size} height={size} viewBox="0 0 512 512" className={className} role="img" aria-label="SSHVault">
      <defs>
        <linearGradient id={`${id}-bg`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#8f72ff" />
          <stop offset="1" stopColor="#4b32e8" />
        </linearGradient>
      </defs>
      <rect width="512" height="512" rx="116" fill={`url(#${id}-bg)`} />
      <path d="M256 92 394 142v112c0 82-58 140-138 172-80-32-138-90-138-172V142z" fill="#fff" fillOpacity="0.13" stroke="#fff" strokeOpacity="0.92" strokeWidth="16" strokeLinejoin="round" />
      <path transform="translate(-14 0)" d="m204 212 58 44-58 44" fill="none" stroke="#fff" strokeWidth="28" strokeLinecap="round" strokeLinejoin="round" />
      <path transform="translate(-14 0)" d="M284 306h52" fill="none" stroke="#fff" strokeWidth="28" strokeLinecap="round" />
    </svg>
  );
}
