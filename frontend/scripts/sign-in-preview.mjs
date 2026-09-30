// Fictional profiles and illustrated portraits for public documentation only.
export const signInProfiles = [
  { username: 'mira', displayName: 'Mira', role: 'admin', profileType: 'adult', authMode: 'password', avatarUrl: '/api/auth/users/1/avatar?v=1' },
  { username: 'oskar', displayName: 'Oskar', role: 'user', profileType: 'adult', authMode: 'password', avatarUrl: '/api/auth/users/2/avatar?v=1' },
  { username: 'nora', displayName: 'Nora', role: 'user', profileType: 'child', authMode: 'pin', avatarUrl: null },
]

export function signInPortrait(id) {
  const mira = id === '1'
  return `<svg xmlns="http://www.w3.org/2000/svg" width="160" height="160" viewBox="0 0 160 160">
    <rect width="160" height="160" fill="${mira ? '#d9cfbd' : '#c8d2c0'}"/>
    <path d="M27 160v-16c0-34 23-48 53-48s53 14 53 48v16" fill="${mira ? '#56624a' : '#692f38'}"/>
    <path d="M${mira ? '43 91V64c0-35 74-35 74 0v40H43' : '44 63c0-39 72-38 72 0v26H44'}" fill="#3b322a"/>
    <rect x="69" y="92" width="22" height="26" rx="9" fill="#c99065"/>
    <ellipse cx="80" cy="71" rx="29" ry="36" fill="#e1ae83"/>
    <path d="${mira ? 'M49 66V51c0-30 63-32 63 2L92 40c-9 16-26 24-43 26' : 'M50 55c-5-28 66-29 63 3L81 43z'}" fill="#3b322a"/>
    <circle cx="69" cy="72" r="2" fill="#3b322a"/><circle cx="91" cy="72" r="2" fill="#3b322a"/>
    <path d="M72 87q8 6 16 0" fill="none" stroke="#8f3c1c" stroke-width="2" stroke-linecap="round"/>
  </svg>`
}
