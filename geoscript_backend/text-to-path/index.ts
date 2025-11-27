import { config } from './conf';
import Session from 'svg-text-to-path';

// Simple LRU cache with bounded size
class LRUCache<T> {
  private maxSize: number;
  private cache: Map<string, T>;

  constructor(maxSize: number) {
    this.maxSize = maxSize;
    this.cache = new Map();
  }

  get(key: string): T | undefined {
    const value = this.cache.get(key);
    if (value !== undefined) {
      // Move to end (most recently used)
      this.cache.delete(key);
      this.cache.set(key, value);
    }
    return value;
  }

  set(key: string, value: T): void {
    // If key exists, delete it first to update position
    if (this.cache.has(key)) {
      this.cache.delete(key);
    } else if (this.cache.size >= this.maxSize) {
      // Evict oldest (first) entry
      const firstKey = this.cache.keys().next().value;
      if (firstKey !== undefined) {
        this.cache.delete(firstKey);
      }
    }
    this.cache.set(key, value);
  }

  get size(): number {
    return this.cache.size;
  }
}

// Cache for successful conversion results (path strings)
// Also cache errors to avoid repeated lookups for invalid fonts
type CacheEntry = { type: 'success'; path: string } | { type: 'error'; error: string; status: number };
const requestCache = new LRUCache<CacheEntry>(5000);

// Generate a cache key from request parameters
function getCacheKey(params: TextToPathRequest): string {
  // Normalize parameters with defaults for consistent hashing
  const normalized = {
    text: params.text,
    fontFamily: params.fontFamily,
    fontSize: params.fontSize ?? 16,
    fontWeight: params.fontWeight ?? 400,
    fontStyle: params.fontStyle ?? 'normal',
    letterSpacing: params.letterSpacing ?? 0,
  };
  return JSON.stringify(normalized);
}

// Types for the API request
interface TextToPathRequest {
  text: string;
  fontFamily: string;
  fontSize?: number;
  fontWeight?: number | string;
  fontStyle?: 'normal' | 'italic' | 'oblique';
  letterSpacing?: number;
}

interface TextToPathResponse {
  path: string;
}

interface ErrorResponse {
  error: string;
}

// Validate and sanitize font family name to prevent injection
function sanitizeFontFamily(family: string): string {
  // Allow alphanumeric, spaces, and common font name characters
  const sanitized = family.replace(/[^a-zA-Z0-9\s\-_]/g, '');
  if (sanitized.length === 0 || sanitized.length > 100) {
    throw new Error('Invalid font family name');
  }
  return sanitized;
}

// Build an SVG with the text element for conversion
function buildSvgWithText(params: TextToPathRequest): string {
  const fontSize = params.fontSize || 16;
  const fontWeight = params.fontWeight || 400;
  const fontStyle = params.fontStyle || 'normal';
  const letterSpacing = params.letterSpacing || 0;

  // Escape XML special characters in text
  const escapedText = params.text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');

  // Calculate viewBox dimensions based on text (rough estimate)
  const width = Math.ceil(escapedText.length * fontSize * 0.8) + 100;
  const height = Math.ceil(fontSize * 2) + 50;

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
  <text x="0" y="${fontSize}"
        font-family="${params.fontFamily}"
        font-size="${fontSize}"
        font-weight="${fontWeight}"
        font-style="${fontStyle}"
        letter-spacing="${letterSpacing}">${escapedText}</text>
</svg>`;
}

// Extract path data from the converted SVG
function extractPathData(svgString: string): string {
  // Match all path d attributes and concatenate them
  const pathRegex = /<path[^>]*\sd="([^"]*)"/g;
  const paths: string[] = [];

  let match;
  while ((match = pathRegex.exec(svgString)) !== null) {
    if (match[1]) {
      paths.push(match[1]);
    }
  }

  return paths.join(' ');
}

// Result from conversion including stats for error detection
interface ConversionResult {
  path: string;
  fontNotFound: boolean;
  missedFamilies: string[];
}

// Main conversion function
async function convertTextToPath(params: TextToPathRequest): Promise<ConversionResult> {
  const svg = buildSvgWithText(params);

  const session = new Session(svg, {
    googleApiKey: config.googleFontsApiKey,
    googleCache: config.googleCacheMs,
    decimals: 2,
  });

  try {
    const stats = await session.replaceAll();
    const resultSvg = session.getSvgString();
    const path = extractPathData(resultSvg);

    // Check if the font was found - if missed array contains our font family
    // or if no paths were generated despite having text, the font wasn't found
    const fontNotFound = stats.missed?.length > 0 || (stats.replaced > 0 && !path);
    const missedFamilies = stats.missed || [];

    return { path, fontNotFound, missedFamilies };
  } finally {
    session.destroy();
  }
}

// Request handler for the text-to-path endpoint
async function handleTextToPath(req: Request): Promise<Response> {
  // Only accept POST requests
  if (req.method !== 'POST') {
    return Response.json({ error: 'Method not allowed. Use POST.' } satisfies ErrorResponse, { status: 405 });
  }

  let body: TextToPathRequest;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'Invalid JSON body' } satisfies ErrorResponse, { status: 400 });
  }

  // Validate required fields
  if (!body.text || typeof body.text !== 'string') {
    return Response.json({ error: 'Missing or invalid "text" field' } satisfies ErrorResponse, {
      status: 400,
    });
  }

  if (!body.fontFamily || typeof body.fontFamily !== 'string') {
    return Response.json({ error: 'Missing or invalid "fontFamily" field' } satisfies ErrorResponse, {
      status: 400,
    });
  }

  // Check text length limit
  if (body.text.length > config.maxTextLength) {
    return Response.json(
      { error: `Text exceeds maximum length of ${config.maxTextLength} characters` } satisfies ErrorResponse,
      { status: 400 }
    );
  }

  // Validate and sanitize font family
  try {
    body.fontFamily = sanitizeFontFamily(body.fontFamily);
  } catch {
    return Response.json({ error: 'Invalid font family name' } satisfies ErrorResponse, { status: 400 });
  }

  // Validate font size if provided
  if (body.fontSize !== undefined) {
    if (typeof body.fontSize !== 'number' || body.fontSize <= 0 || body.fontSize > config.maxFontSize) {
      return Response.json(
        {
          error: `Font size must be a positive number not exceeding ${config.maxFontSize}`,
        } satisfies ErrorResponse,
        { status: 400 }
      );
    }
  }

  // Validate font weight if provided
  if (body.fontWeight !== undefined) {
    const validWeights = [100, 200, 300, 400, 500, 600, 700, 800, 900, 'normal', 'bold'];
    if (!validWeights.includes(body.fontWeight)) {
      return Response.json(
        { error: 'Invalid font weight. Use 100-900 or "normal"/"bold"' } satisfies ErrorResponse,
        { status: 400 }
      );
    }
  }

  // Validate font style if provided
  if (body.fontStyle !== undefined) {
    const validStyles = ['normal', 'italic', 'oblique'];
    if (!validStyles.includes(body.fontStyle)) {
      return Response.json(
        { error: 'Invalid font style. Use "normal", "italic", or "oblique"' } satisfies ErrorResponse,
        { status: 400 }
      );
    }
  }

  // Check cache first
  const cacheKey = getCacheKey(body);
  const cached = requestCache.get(cacheKey);
  if (cached) {
    if (cached.type === 'success') {
      return Response.json({ path: cached.path } satisfies TextToPathResponse);
    } else {
      return Response.json({ error: cached.error } satisfies ErrorResponse, { status: cached.status });
    }
  }

  try {
    const result = await convertTextToPath(body);

    // Check if font wasn't found
    if (result.fontNotFound) {
      const missedList =
        result.missedFamilies.length > 0 ? result.missedFamilies.join(', ') : body.fontFamily;
      const errorMsg = `Font not found: ${missedList}. Ensure the font is available on Google Fonts.`;
      // Cache the font-not-found error
      requestCache.set(cacheKey, { type: 'error', error: errorMsg, status: 400 });
      return Response.json({ error: errorMsg } satisfies ErrorResponse, { status: 400 });
    }

    if (!result.path) {
      console.error('No path data generated for request:', body);
      // Don't cache this error - it might be transient
      return Response.json(
        {
          error: 'Failed to generate path. The text may contain unsupported characters.',
        } satisfies ErrorResponse,
        { status: 500 }
      );
    }

    // Cache successful result
    requestCache.set(cacheKey, { type: 'success', path: result.path });
    return Response.json({ path: result.path } satisfies TextToPathResponse);
  } catch (err) {
    console.error('Text to path conversion error:', err);
    return Response.json({ error: 'Internal server error during conversion' } satisfies ErrorResponse, {
      status: 500,
    });
  }
}

// CORS headers for all responses
const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': '*',
};

// Wrap response with CORS headers
function withCors(response: Response): Response {
  const newHeaders = new Headers(response.headers);
  for (const [key, value] of Object.entries(corsHeaders)) {
    newHeaders.set(key, value);
  }
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: newHeaders,
  });
}

// Handle CORS preflight
function handleOptions(): Response {
  return new Response(null, { status: 204, headers: corsHeaders });
}

// Wrapped handler with CORS
async function handleTextToPathWithCors(req: Request): Promise<Response> {
  return withCors(await handleTextToPath(req));
}

const server = Bun.serve({
  port: config.port,
  routes: {
    // Health check endpoint
    '/api/health': new Response('OK', { headers: corsHeaders }),

    // Main text-to-path conversion endpoint
    '/api/text-to-path': {
      OPTIONS: handleOptions,
      POST: handleTextToPathWithCors,
    },

    // Catch-all for /api/* routes not matched
    '/api/*': Response.json({ error: 'Not found' } satisfies ErrorResponse, {
      status: 404,
      headers: corsHeaders,
    }),
  },

  // Fallback for non-API routes
  fetch() {
    return Response.json({ error: 'Not found' } satisfies ErrorResponse, {
      status: 404,
      headers: corsHeaders,
    });
  },
});

console.log(`Text-to-path server running at ${server.url}`);
