/**
 * IP geolocation via the free ip-api.com service.
 *
 * Rate-limited to 45 requests per minute on the free tier.
 * Results are cached in-memory for the current session to avoid
 * redundant lookups.
 */

export interface GeoLocation {
  lat: number;
  lng: number;
  city?: string;
  region?: string;
  country?: string;
}

const cache = new Map<string, GeoLocation | null>();

/**
 * Resolve an IP address (v4 or v6) to geographic coordinates.
 *
 * Returns `null` if the lookup fails or the IP is a private/reserved
 * address that cannot be geolocated.
 */
export async function geolocateIp(ip: string): Promise<GeoLocation | null> {
  const key = ip.trim();
  if (cache.has(key)) return cache.get(key)!;

  try {
    const res = await fetch(
      `http://ip-api.com/json/${encodeURIComponent(key)}?fields=status,lat,lon,city,regionName,country`,
    );
    if (!res.ok) {
      cache.set(key, null);
      return null;
    }

    const data = (await res.json()) as {
      status: string;
      lat?: number;
      lon?: number;
      city?: string;
      regionName?: string;
      country?: string;
    };

    if (data.status !== "success" || data.lat == null || data.lon == null) {
      cache.set(key, null);
      return null;
    }

    const geo: GeoLocation = {
      lat: data.lat,
      lng: data.lon,
      city: data.city,
      region: data.regionName,
      country: data.country,
    };
    cache.set(key, geo);
    return geo;
  } catch {
    cache.set(key, null);
    return null;
  }
}
