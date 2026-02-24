export interface MediaAttachment {
  hash: string;
  ticket: string;
  mime_type: string;
  filename: string;
  size: number;
}

export interface Post {
  id: string;
  author: string;
  content: string;
  timestamp: number;
  media: MediaAttachment[];
}

export interface PendingAttachment {
  hash: string;
  ticket: string;
  mime_type: string;
  filename: string;
  size: number;
  previewUrl: string;
}

export interface Profile {
  display_name: string;
  bio: string;
  avatar_hash: string | null;
  avatar_ticket: string | null;
}

export interface FollowEntry {
  pubkey: string;
  alias: string | null;
  followed_at: number;
}

export interface FollowerEntry {
  pubkey: string;
  first_seen: number;
  last_seen: number;
  is_online: boolean;
}

export interface NodeStatus {
  node_id: string;
  has_relay: boolean;
  relay_url: string | null;
  follow_count: number;
  follower_count: number;
}
