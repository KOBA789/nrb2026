// API レスポンスの型 (docs/idea.md 仕様)。

export interface Participant {
  user_id: string;
  name: string;
  joined_at: string;
}

export interface Campaign {
  id: string;
  name: string;
  description: string;
  price: number;
  goal_count: number;
  current_count: number;
  tags: string[];
  status: "open" | "closed";
  created_at: string;
  last_joined_at: string | null;
  participants: Participant[];
}

export interface MeRes {
  id: string;
  name: string;
  credit_limit: number;
  credit_used: number;
}

export interface UserRes {
  id: string;
  name: string;
  credit_limit: number;
}

export interface ChargeRes {
  id: string;
  amount: number;
  campaign: { id: string; name: string; price: number };
  created_at: string;
}
