import type { Config } from 'tailwindcss';

export default {
  darkMode: ['class'],
  content: ['./src/**/*.{ts,tsx}', './index.html'],
  theme: {
    container: {
      center: true,
      padding: '2rem',
      screens: {
        '2xl': '1400px',
      },
    },
    extend: {
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
          50:  '#fff8e6',
          100: '#feefc0',
          200: '#fde38a',
          300: '#fcd154',
          400: '#fbc22e',
          500: '#f9b233',
          600: '#e09520',
          700: '#b87415',
          800: '#8f5610',
          900: '#6b3d0c',
          950: '#3d2004',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))',
          400: '#a855f7',
          500: '#9200d4',
          600: '#7600a0',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
          50: '#fffbeb',
          100: '#fef3c7',
          200: '#fde68a',
          300: '#fcd34d',
          400: '#fbbf24',
          500: '#f59e0b',
          600: '#d97706',
          700: '#b45309',
          800: '#92400e',
          900: '#78350f',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))',
        },
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))',
        },
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
      },
      backgroundImage: {
        'gradient-radial': 'radial-gradient(var(--tw-gradient-stops))',
        'gradient-conic': 'conic-gradient(from 180deg at 50% 50%, var(--tw-gradient-stops))',
        'gradient-primary': 'linear-gradient(135deg, #e09520 0%, #f9b233 100%)',
        'gradient-secondary': 'linear-gradient(135deg, #6d007f 0%, #9200d4 100%)',
        'gradient-accent': 'linear-gradient(135deg, #f59e0b 0%, #fbbf24 100%)',
        'gradient-mesh':
          'radial-gradient(at 27% 37%, hsla(271, 76%, 53%, 1) 0px, transparent 50%), radial-gradient(at 97% 21%, hsla(186, 78%, 44%, 1) 0px, transparent 50%), radial-gradient(at 52% 99%, hsla(38, 92%, 50%, 1) 0px, transparent 50%), radial-gradient(at 10% 29%, hsla(271, 76%, 53%, 1) 0px, transparent 50%), radial-gradient(at 97% 96%, hsla(186, 78%, 44%, 1) 0px, transparent 50%), radial-gradient(at 33% 50%, hsla(38, 92%, 50%, 1) 0px, transparent 50%), radial-gradient(at 79% 53%, hsla(271, 76%, 53%, 1) 0px, transparent 50%)',
      },
      keyframes: {
        'accordion-down': {
          from: { height: '0' },
          to: { height: 'var(--radix-accordion-content-height)' },
        },
        'accordion-up': {
          from: { height: 'var(--radix-accordion-content-height)' },
          to: { height: '0' },
        },
        'glow': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.5' },
        },
        'slide-up': {
          '0%': { transform: 'translateY(100%)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
        'fade-in': {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        'ambientPulse': {
          '0%, 100%': { opacity: '0.45', transform: 'scale(1)' },
          '50%': { opacity: '0.6', transform: 'scale(1.04)' },
        },
        'ambientDrift': {
          '0%, 100%': { opacity: '0.25', transform: 'scale(1) rotate(0deg)' },
          '33%': { opacity: '0.35', transform: 'scale(1.06) rotate(1.5deg)' },
          '66%': { opacity: '0.2', transform: 'scale(0.97) rotate(-1deg)' },
        },
        'ambientBreath': {
          '0%, 100%': { opacity: '0.15', transform: 'scale(1)' },
          '50%': { opacity: '0.25', transform: 'scale(1.08)' },
        },
        'vinylSpin': {
          '0%': { transform: 'rotate(0deg)' },
          '100%': { transform: 'rotate(360deg)' },
        },
      },
      animation: {
        'accordion-down': 'accordion-down 0.2s ease-out',
        'accordion-up': 'accordion-up 0.2s ease-out',
        'glow': 'glow 2s ease-in-out infinite',
        'slide-up': 'slide-up 0.5s ease-out',
        'fade-in': 'fade-in 0.3s ease-out',
        'ambient-pulse': 'ambientPulse 4s ease-in-out infinite',
        'ambient-drift': 'ambientDrift 6s ease-in-out infinite',
        'ambient-breath': 'ambientBreath 5s ease-in-out infinite',
        'vinyl-spin': 'vinylSpin 10s linear infinite',
      },
      boxShadow: {
        'glow-amber': '0 0 20px rgba(249, 178, 51, 0.4)',
        'glow-teal': '0 0 20px rgba(41, 171, 135, 0.4)',
        'glow-purple': '0 0 20px rgba(146, 0, 212, 0.4)',
        'glow-orange': '0 0 20px rgba(245, 158, 11, 0.5)',
      },
    },
  },
  plugins: [require('tailwindcss-animate')],
} satisfies Config;
