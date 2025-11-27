// Configuration loaded from environment variables

export const config = {
  port: parseInt(process.env.PORT || '3000', 10),
  googleFontsApiKey: process.env.GOOGLE_FONTS_API_KEY || '',
  // Cache duration for Google Fonts API responses (10 minutes default)
  googleCacheMs: parseInt(process.env.GOOGLE_CACHE_MS || '600000', 10),
  // Maximum input text length to prevent abuse
  maxTextLength: parseInt(process.env.MAX_TEXT_LENGTH || '10000', 10),
  // Maximum font size to prevent abuse
  maxFontSize: parseInt(process.env.MAX_FONT_SIZE || '1000', 10),
};

// Validate required config
if (!config.googleFontsApiKey) {
  console.warn('Warning: GOOGLE_FONTS_API_KEY not set. Google Fonts lookups will fail.');
}
