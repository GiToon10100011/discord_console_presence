import { useId } from "react";

/**
 * The app logo: a game controller with the green "online" dot of a Rich
 * Presence. It is the same artwork as the app icon
 * (src-tauri/icons/icon.svg), so the header and the dock/taskbar match.
 */
export function Logo({ className }: { className?: string }) {
  const gradient = useId().replace(/:/g, "");
  return (
    <svg
      viewBox="100 100 824 824"
      className={className}
      role="img"
      aria-label="Console Presence"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id={gradient} x1="140" y1="100" x2="880" y2="930" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#7B86FF" />
          <stop offset="1" stopColor="#3D47B5" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="188" fill={`url(#${gradient})`} />
      <g transform="translate(176 160) scale(28)">
        <path
          d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z"
          fill="#FFFFFF"
          stroke="#FFFFFF"
          strokeWidth="0.6"
          strokeLinejoin="round"
        />
        <path d="M6 11h4M8 9v4" stroke="#434DBD" strokeWidth="1.6" strokeLinecap="round" fill="none" />
        <circle cx="15" cy="12" r="1.1" fill="#434DBD" />
        <circle cx="18" cy="10" r="1.1" fill="#434DBD" />
      </g>
      <circle cx="790" cy="752" r="100" fill="#3F49B9" />
      <circle cx="790" cy="752" r="74" fill="#23A559" />
    </svg>
  );
}
