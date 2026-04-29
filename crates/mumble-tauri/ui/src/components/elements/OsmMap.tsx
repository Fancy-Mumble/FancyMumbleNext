/**
 * Reusable OpenStreetMap component backed by react-leaflet.
 *
 * Displays a tile map centered on the given coordinates with a marker.
 * Can be used wherever a location needs to be shown (user connection
 * info, message locations, etc.).
 */

import { useEffect, useRef } from "react";
import { MapContainer, TileLayer, Marker, Popup, useMap } from "react-leaflet";
import { icon } from "leaflet";
import "leaflet/dist/leaflet.css";
import styles from "./OsmMap.module.css";

import markerIcon from "leaflet/dist/images/marker-icon.png";
import markerIcon2x from "leaflet/dist/images/marker-icon-2x.png";
import markerShadow from "leaflet/dist/images/marker-shadow.png";

const defaultIcon = icon({
  iconUrl: markerIcon,
  iconRetinaUrl: markerIcon2x,
  shadowUrl: markerShadow,
  iconSize: [25, 41],
  iconAnchor: [12, 41],
  popupAnchor: [1, -34],
  shadowSize: [41, 41],
});

export interface OsmMapProps {
  /** Latitude of the center / marker position. */
  lat: number;
  /** Longitude of the center / marker position. */
  lng: number;
  /** Zoom level (default: 10). */
  zoom?: number;
  /** Optional popup label shown when clicking the marker. */
  popupLabel?: string;
  /** Optional CSS class for the container. */
  className?: string;
}

function MapUpdater({ lat, lng, zoom }: { lat: number; lng: number; zoom: number }) {
  const map = useMap();
  useEffect(() => {
    map.setView([lat, lng], zoom);
  }, [map, lat, lng, zoom]);
  return null;
}

export default function OsmMap({
  lat,
  lng,
  zoom = 10,
  popupLabel,
  className,
}: Readonly<OsmMapProps>) {
  const wrapperRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = wrapperRef.current;
    if (!el) return;
    const stopScroll = (e: WheelEvent) => e.stopPropagation();
    el.addEventListener("wheel", stopScroll, { passive: false });
    return () => el.removeEventListener("wheel", stopScroll);
  }, []);

  return (
    <div ref={wrapperRef} className={className ?? styles.mapContainer}>
      <MapContainer
        center={[lat, lng]}
        zoom={zoom}
        scrollWheelZoom={true}
        dragging={true}
        zoomControl={true}
        attributionControl={true}
        style={{ width: "100%", height: "100%" }}
      >
        <MapUpdater lat={lat} lng={lng} zoom={zoom} />
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
          url="https://tile.openstreetmap.org/{z}/{x}/{y}.png"
        />
        <Marker position={[lat, lng]} icon={defaultIcon}>
          {popupLabel && <Popup>{popupLabel}</Popup>}
        </Marker>
      </MapContainer>
    </div>
  );
}
