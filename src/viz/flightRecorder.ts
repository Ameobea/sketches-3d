import flightRecorderWasmURL from 'src/viz/wasmComp/flight_recorder.wasm?url';

export const enum RecorderEventType {
  Jump = 0,
  Dash = 1,
  ZoneEvent = 2,
  Teleport = 3,
  RunStart = 4,
  RunEnd = 5,
  OOBRespawn = 6,
  Pause = 7,
  Unpause = 8,
  SettingsChanged = 9,
}

interface FlightRecorderExports {
  memory: WebAssembly.Memory;
  create_recorder: () => number;
  destroy_recorder: (ctx: number) => void;
  fr_malloc: (size: number) => number;
  fr_free: (ptr: number, size: number) => void;
  set_header: (ctx: number, headerPtr: number, len: number) => void;
  set_metadata_string: (ctx: number, keyPtr: number, keyLen: number, valPtr: number, valLen: number) => void;
  record_subtick: (ctx: number, physicsStatePtr: number, inputStatePtr: number, keyFlags: number) => void;
  record_event: (ctx: number, eventType: number, dataPtr: number, dataLen: number) => void;
  serialize: (ctx: number) => number;
  get_serialized_len: (ctx: number) => number;
  reset_recorder: (ctx: number) => void;
  get_subtick_count: (ctx: number) => number;

  // Player (deserialization) exports
  create_player: () => number;
  destroy_player: (ctx: number) => void;
  player_load: (ctx: number, dataPtr: number, len: number) => number;
  player_get_header: (ctx: number, outPtr: number) => void;
  player_get_metadata: (
    ctx: number,
    keyPtr: number,
    keyLen: number,
    outPtr: number,
    outCapacity: number
  ) => number;
  player_get_subtick: (ctx: number, index: number, outPtr: number) => number;
  player_get_subtick_count: (ctx: number) => number;
  player_get_event_count: (ctx: number) => number;
  player_get_event: (ctx: number, index: number, outPtr: number) => number;
}

export function packKeyFlags(keyStates: Record<string, boolean>): number {
  let flags = 0;
  if (keyStates['KeyW']) flags |= 1;
  if (keyStates['KeyS']) flags |= 2;
  if (keyStates['KeyA']) flags |= 4;
  if (keyStates['KeyD']) flags |= 8;
  if (keyStates['Space']) flags |= 16;
  if (keyStates['ShiftLeft'] || keyStates['ShiftRight']) flags |= 32;
  return flags;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

const loadFlightRecorderInstance = async (): Promise<FlightRecorderExports> => {
  const response = await fetch(flightRecorderWasmURL);
  const bytes = await response.arrayBuffer();
  const module = await WebAssembly.compile(bytes);
  const instance = await WebAssembly.instantiate(module, { env: {} });
  return instance.exports as unknown as FlightRecorderExports;
};

export class FlightRecorder {
  private exports!: FlightRecorderExports;
  private ctx = 0;
  private physicsStatePtr = 0;
  private inputStatePtr = 0;
  private headerPtr = 0;
  private eventDataPtr = 0;
  private ready = false;

  async init(): Promise<void> {
    this.exports = await loadFlightRecorderInstance();

    this.ctx = this.exports.create_recorder();

    // Allocate persistent scratch buffers in Wasm memory
    this.physicsStatePtr = this.exports.fr_malloc(40); // 10 f32s
    this.inputStatePtr = this.exports.fr_malloc(24); // 6 f32s
    this.headerPtr = this.exports.fr_malloc(132); // 33 f32s
    this.eventDataPtr = this.exports.fr_malloc(32); // 8 f32s

    this.ready = true;
  }

  get isReady(): boolean {
    return this.ready;
  }

  setHeader(header: {
    tickRateHz: number;
    gravity: number;
    jumpSpeed: number;
    moveSpeedGround: number;
    moveSpeedAir: number;
    colliderHeight: number;
    colliderRadius: number;
    extVelAirDamping: [number, number, number];
    extVelGroundDamping: [number, number, number];
    gravityShapeRiseMult: number;
    gravityShapeApexMult: number;
    gravityShapeFallMult: number;
    gravityShapeApexThreshold: number;
    gravityShapeKneeWidth: number;
    gravityShapeOnlyJumps: boolean;
    stepHeight: number;
    terminalVelocity: number;
    maxSlopeRadians: number;
    maxPenetrationDepth: number;
    coyoteTimeSeconds: number;
    minJumpDelaySeconds: number;
    easyModeMovement: boolean;
    colliderShape: 'capsule' | 'cylinder' | 'sphere';
    dashEnabled: boolean;
    dashMagnitude: number;
    minDashDelaySeconds: number;
    dashUseExternalVelocity: boolean;
    mapIdHash?: bigint;
  }): void {
    if (!this.ready) return;

    const HEADER_FLOATS = 33;
    const view = new Float32Array(this.exports.memory.buffer, this.headerPtr, HEADER_FLOATS);
    view[0] = header.tickRateHz;
    view[1] = header.gravity;
    view[2] = header.jumpSpeed;
    view[3] = header.moveSpeedGround;
    view[4] = header.moveSpeedAir;
    view[5] = header.colliderHeight;
    view[6] = header.colliderRadius;
    view[7] = header.extVelAirDamping[0];
    view[8] = header.extVelAirDamping[1];
    view[9] = header.extVelAirDamping[2];
    view[10] = header.extVelGroundDamping[0];
    view[11] = header.extVelGroundDamping[1];
    view[12] = header.extVelGroundDamping[2];
    view[13] = header.gravityShapeRiseMult;
    view[14] = header.gravityShapeApexMult;
    view[15] = header.gravityShapeFallMult;
    view[16] = header.gravityShapeApexThreshold;
    view[17] = header.gravityShapeKneeWidth;
    view[18] = header.gravityShapeOnlyJumps ? 1.0 : 0.0;

    if (header.mapIdHash !== undefined) {
      const lo = Number(header.mapIdHash & 0xffffffffn);
      const hi = Number((header.mapIdHash >> 32n) & 0xffffffffn);
      const dv = new DataView(this.exports.memory.buffer, this.headerPtr, HEADER_FLOATS * 4);
      dv.setUint32(19 * 4, lo, true);
      dv.setUint32(20 * 4, hi, true);
    } else {
      view[19] = 0;
      view[20] = 0;
    }

    view[21] = header.stepHeight;
    view[22] = header.terminalVelocity;
    view[23] = header.maxSlopeRadians;
    view[24] = header.maxPenetrationDepth;
    view[25] = header.coyoteTimeSeconds;
    view[26] = header.minJumpDelaySeconds;
    view[27] = header.easyModeMovement ? 1.0 : 0.0;
    const shapeMap = { capsule: 0, cylinder: 1, sphere: 2 } as const;
    view[28] = shapeMap[header.colliderShape];
    view[29] = header.dashEnabled ? 1.0 : 0.0;
    view[30] = header.dashMagnitude;
    view[31] = header.minDashDelaySeconds;
    view[32] = header.dashUseExternalVelocity ? 1.0 : 0.0;

    this.exports.set_header(this.ctx, this.headerPtr, HEADER_FLOATS);
  }

  setMetadataString(key: string, value: string): void {
    if (!this.ready) return;

    const keyBytes = textEncoder.encode(key);
    const valBytes = textEncoder.encode(value);

    const keyPtr = this.exports.fr_malloc(keyBytes.byteLength);
    const valPtr = this.exports.fr_malloc(valBytes.byteLength);

    new Uint8Array(this.exports.memory.buffer, keyPtr, keyBytes.byteLength).set(keyBytes);
    new Uint8Array(this.exports.memory.buffer, valPtr, valBytes.byteLength).set(valBytes);

    this.exports.set_metadata_string(this.ctx, keyPtr, keyBytes.byteLength, valPtr, valBytes.byteLength);

    this.exports.fr_free(keyPtr, keyBytes.byteLength);
    this.exports.fr_free(valPtr, valBytes.byteLength);
  }

  /**
   * Record a subtick snapshot. Called once per physics substep.
   *
   * @param ammoStatePtr  Pointer into Ammo's HEAPF32 where packState wrote 10 floats
   * @param ammoHeapF32   Ammo's HEAPF32 buffer
   * @param walkDirX/Y/Z  The walk direction applied this subtick
   * @param phi           Camera phi angle
   * @param theta         Camera theta angle
   * @param zoomDistance  Third-person camera zoom distance (0 in first-person)
   * @param keyFlags      Packed key bitmask from packKeyFlags()
   */
  recordSubtick(
    ammoStatePtr: number,
    ammoHeapF32: Float32Array,
    walkDirX: number,
    walkDirY: number,
    walkDirZ: number,
    phi: number,
    theta: number,
    zoomDistance: number,
    keyFlags: number
  ): void {
    if (!this.ready) return;

    // Copy 10 floats from Ammo's heap into our Wasm's heap
    const physView = new Float32Array(this.exports.memory.buffer, this.physicsStatePtr, 10);
    const ammoFloatOffset = ammoStatePtr / 4; // byte offset to float index
    for (let i = 0; i < 10; i++) {
      physView[i] = ammoHeapF32[ammoFloatOffset + i];
    }

    // Write input state
    const inputView = new Float32Array(this.exports.memory.buffer, this.inputStatePtr, 6);
    inputView[0] = walkDirX;
    inputView[1] = walkDirY;
    inputView[2] = walkDirZ;
    inputView[3] = phi;
    inputView[4] = theta;
    inputView[5] = zoomDistance;

    this.exports.record_subtick(this.ctx, this.physicsStatePtr, this.inputStatePtr, keyFlags);
  }

  recordEvent(eventType: RecorderEventType, data?: number[]): void {
    if (!this.ready) return;

    const len = data?.length ?? 0;
    if (data && len > 0) {
      const view = new Float32Array(this.exports.memory.buffer, this.eventDataPtr, 8);
      for (let i = 0; i < Math.min(len, 8); i++) {
        view[i] = data[i];
      }
    }

    this.exports.record_event(this.ctx, eventType, len > 0 ? this.eventDataPtr : 0, len);
  }

  serialize(): Uint8Array | null {
    if (!this.ready) return null;

    const ptr = this.exports.serialize(this.ctx);
    const len = this.exports.get_serialized_len(this.ctx);
    if (len === 0) return null;

    // Copy from Wasm memory so the caller owns the buffer
    return new Uint8Array(this.exports.memory.buffer, ptr, len).slice();
  }

  reset(): void {
    if (!this.ready) return;
    this.exports.reset_recorder(this.ctx);
  }

  get subtickCount(): number {
    if (!this.ready) return 0;
    return this.exports.get_subtick_count(this.ctx);
  }

  destroy(): void {
    if (!this.ready) return;
    this.exports.fr_free(this.physicsStatePtr, 40);
    this.exports.fr_free(this.inputStatePtr, 24);
    this.exports.fr_free(this.headerPtr, 132);
    this.exports.fr_free(this.eventDataPtr, 32);
    this.exports.destroy_recorder(this.ctx);
    this.ready = false;
  }
}

export interface ReplayHeader {
  tickRateHz: number;
  gravity: number;
  jumpSpeed: number;
  moveSpeedGround: number;
  moveSpeedAir: number;
  colliderHeight: number;
  colliderRadius: number;
  extVelAirDamping: [number, number, number];
  extVelGroundDamping: [number, number, number];
  gravityShapeRiseMult: number;
  gravityShapeApexMult: number;
  gravityShapeFallMult: number;
  gravityShapeApexThreshold: number;
  gravityShapeKneeWidth: number;
  gravityShapeOnlyJumps: boolean;
  stepHeight: number;
  terminalVelocity: number;
  maxSlopeRadians: number;
  maxPenetrationDepth: number;
  coyoteTimeSeconds: number;
  minJumpDelaySeconds: number;
  easyModeMovement: boolean;
  colliderShape: number;
  dashEnabled: boolean;
  dashMagnitude: number;
  minDashDelaySeconds: number;
  dashUseExternalVelocity: boolean;
}

export interface ReplayHeaderMismatch {
  field: keyof ReplayValidationConfig;
  recorded: number | boolean | string;
  current: number | boolean | string;
  absDiff?: number;
}

export interface SubtickInput {
  walkDir: [number, number, number];
  phi: number;
  theta: number;
  zoomDistance: number;
  keyFlags: number;
}

/** Recorded physics state from a subtick (from C++ packState, 10 floats). */
export interface SubtickPhysicsSnapshot {
  pos: [number, number, number];
  externalVel: [number, number, number];
  verticalVel: number;
  verticalOffset: number;
  onGround: boolean;
  isJumping: boolean;
  floorUserIndex: number;
}

export interface ReplayEvent {
  type: RecorderEventType;
  data: number[];
}

export class FlightPlayer {
  private exports!: FlightRecorderExports;
  private ctx = 0;
  private subtickPtr = 0; // 16 f32s
  private eventPtr = 0; // 6 f32s
  private headerPtr = 0; // 33 f32s
  private metadataOutPtr = 0; // scratch buffer for metadata reads
  private ready = false;

  // Pre-built event index: eventsBySubtick[subtickIndex] = array of events
  private eventsBySubtick: Map<number, ReplayEvent[]> = new Map();

  private freeScratchBuffers(): void {
    if (!this.exports) return;
    if (this.subtickPtr !== 0) {
      this.exports.fr_free(this.subtickPtr, 68);
      this.subtickPtr = 0;
    }
    if (this.eventPtr !== 0) {
      this.exports.fr_free(this.eventPtr, 40);
      this.eventPtr = 0;
    }
    if (this.headerPtr !== 0) {
      this.exports.fr_free(this.headerPtr, 132);
      this.headerPtr = 0;
    }
    if (this.metadataOutPtr !== 0) {
      this.exports.fr_free(this.metadataOutPtr, 1024);
      this.metadataOutPtr = 0;
    }
  }

  async load(replayData: Uint8Array): Promise<boolean> {
    this.exports = await loadFlightRecorderInstance();

    this.ctx = this.exports.create_player();

    // Allocate scratch buffers
    this.subtickPtr = this.exports.fr_malloc(68); // 17 f32s
    this.eventPtr = this.exports.fr_malloc(40); // 10 f32s (type + subtick + 8 data)
    this.headerPtr = this.exports.fr_malloc(132); // 33 f32s
    this.metadataOutPtr = this.exports.fr_malloc(1024); // 1KB scratch for metadata reads

    // Copy replay data into Wasm memory and load
    const dataPtr = this.exports.fr_malloc(replayData.byteLength);
    const wasmBuf = new Uint8Array(this.exports.memory.buffer, dataPtr, replayData.byteLength);
    wasmBuf.set(replayData);

    const result = this.exports.player_load(this.ctx, dataPtr, replayData.byteLength);
    this.exports.fr_free(dataPtr, replayData.byteLength);

    if (result !== 0) {
      this.freeScratchBuffers();
      this.exports.destroy_player(this.ctx);
      this.ctx = 0;
      return false;
    }

    // Build event index
    this.buildEventIndex();

    this.ready = true;
    return true;
  }

  private buildEventIndex(): void {
    const count = this.exports.player_get_event_count(this.ctx);

    for (let i = 0; i < count; i++) {
      this.exports.player_get_event(this.ctx, i, this.eventPtr);
      // Re-wrap view in case memory grew
      const v = new Float32Array(this.exports.memory.buffer, this.eventPtr, 10);
      const dv = new DataView(this.exports.memory.buffer, this.eventPtr, 40);
      const eventType = dv.getUint32(0, true) as RecorderEventType;
      const subtick = dv.getUint32(4, true);
      const data = [v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9]];

      let arr = this.eventsBySubtick.get(subtick);
      if (!arr) {
        arr = [];
        this.eventsBySubtick.set(subtick, arr);
      }
      arr.push({ type: eventType, data });
    }
  }

  getHeader(): ReplayHeader {
    this.exports.player_get_header(this.ctx, this.headerPtr);
    const v = new Float32Array(this.exports.memory.buffer, this.headerPtr, 33);
    return {
      tickRateHz: v[0],
      gravity: v[1],
      jumpSpeed: v[2],
      moveSpeedGround: v[3],
      moveSpeedAir: v[4],
      colliderHeight: v[5],
      colliderRadius: v[6],
      extVelAirDamping: [v[7], v[8], v[9]],
      extVelGroundDamping: [v[10], v[11], v[12]],
      gravityShapeRiseMult: v[13],
      gravityShapeApexMult: v[14],
      gravityShapeFallMult: v[15],
      gravityShapeApexThreshold: v[16],
      gravityShapeKneeWidth: v[17],
      gravityShapeOnlyJumps: v[18] > 0.5,
      stepHeight: v[21],
      terminalVelocity: v[22],
      maxSlopeRadians: v[23],
      maxPenetrationDepth: v[24],
      coyoteTimeSeconds: v[25],
      minJumpDelaySeconds: v[26],
      easyModeMovement: v[27] > 0.5,
      colliderShape: v[28],
      dashEnabled: v[29] > 0.5,
      dashMagnitude: v[30],
      minDashDelaySeconds: v[31],
      dashUseExternalVelocity: v[32] > 0.5,
    };
  }

  get subtickCount(): number {
    if (!this.ready) return 0;
    return this.exports.player_get_subtick_count(this.ctx);
  }

  get eventCount(): number {
    if (!this.ready) return 0;
    return this.exports.player_get_event_count(this.ctx);
  }

  getMetadataString(key: string): string | null {
    if (!this.ready) return null;

    const keyBytes = textEncoder.encode(key);
    const keyPtr = this.exports.fr_malloc(keyBytes.byteLength);
    new Uint8Array(this.exports.memory.buffer, keyPtr, keyBytes.byteLength).set(keyBytes);

    const valLen = this.exports.player_get_metadata(
      this.ctx,
      keyPtr,
      keyBytes.byteLength,
      this.metadataOutPtr,
      1024
    );
    this.exports.fr_free(keyPtr, keyBytes.byteLength);

    if (valLen < 0) return null;

    const readLen = Math.min(valLen, 1024);
    const valBytes = new Uint8Array(this.exports.memory.buffer, this.metadataOutPtr, readLen);
    return textDecoder.decode(valBytes);
  }

  getSubtick(index: number): SubtickInput {
    const result = this.exports.player_get_subtick(this.ctx, index, this.subtickPtr);
    if (result !== 0) {
      throw new Error(`Subtick index ${index} out of range`);
    }
    const v = new Float32Array(this.exports.memory.buffer, this.subtickPtr, 17);
    const dv = new DataView(this.exports.memory.buffer, this.subtickPtr, 68);
    return {
      walkDir: [v[0], v[1], v[2]],
      phi: v[3],
      theta: v[4],
      zoomDistance: v[5],
      keyFlags: dv.getUint32(6 * 4, true),
    };
  }

  getSubtickPos(index: number): [number, number, number] {
    const result = this.exports.player_get_subtick(this.ctx, index, this.subtickPtr);
    if (result !== 0) {
      throw new Error(`Subtick index ${index} out of range`);
    }
    const v = new Float32Array(this.exports.memory.buffer, this.subtickPtr, 17);
    return [v[7], v[8], v[9]];
  }

  /**
   * Get the recorded physics snapshot for a subtick.
   * Layout from Rust player_get_subtick: v[7..16] = physics state from packState.
   */
  getSubtickPhysicsSnapshot(index: number): SubtickPhysicsSnapshot {
    const result = this.exports.player_get_subtick(this.ctx, index, this.subtickPtr);
    if (result !== 0) {
      throw new Error(`Subtick index ${index} out of range`);
    }
    const v = new Float32Array(this.exports.memory.buffer, this.subtickPtr, 17);
    const dv = new DataView(this.exports.memory.buffer, this.subtickPtr, 68);
    // v[15] is flags bitcast from u32: bit0=onGround, bit1=isJumping
    const flags = dv.getUint32(15 * 4, true);
    // v[16] is floor_user_index bitcast from i32
    const floorUserIndex = dv.getInt32(16 * 4, true);
    return {
      pos: [v[7], v[8], v[9]],
      externalVel: [v[10], v[11], v[12]],
      verticalVel: v[13],
      verticalOffset: v[14],
      onGround: (flags & 1) !== 0,
      isJumping: (flags & 2) !== 0,
      floorUserIndex,
    };
  }

  getEventsAtSubtick(subtick: number): ReplayEvent[] {
    return this.eventsBySubtick.get(subtick) ?? [];
  }

  destroy(): void {
    if (!this.ready) return;
    this.freeScratchBuffers();
    this.exports.destroy_player(this.ctx);
    this.ctx = 0;
    this.ready = false;
    this.eventsBySubtick.clear();
  }
}

// ─── Replay Validation ──────────────────────────────────────────────────

/** Current scene config values needed for replay header validation. */
export interface ReplayValidationConfig {
  tickRateHz: number;
  gravity: number;
  jumpSpeed: number;
  moveSpeedGround: number;
  moveSpeedAir: number;
  colliderHeight: number;
  colliderRadius: number;
  extVelAirDamping: [number, number, number];
  extVelGroundDamping: [number, number, number];
  gravityShapeRiseMult: number;
  gravityShapeApexMult: number;
  gravityShapeFallMult: number;
  gravityShapeApexThreshold: number;
  gravityShapeKneeWidth: number;
  gravityShapeOnlyJumps: boolean;
  stepHeight: number;
  terminalVelocity: number;
  maxSlopeRadians: number;
  maxPenetrationDepth: number;
  coyoteTimeSeconds: number;
  minJumpDelaySeconds: number;
  easyModeMovement: boolean;
  colliderShape: number;
  dashEnabled: boolean;
  dashMagnitude: number;
  minDashDelaySeconds: number;
  dashUseExternalVelocity: boolean;
}

/**
 * Validate a replay header against current scene config.
 * Returns an array of field mismatches. Empty means all good.
 */
export function validateReplayHeader(
  header: ReplayHeader,
  config: ReplayValidationConfig
): ReplayHeaderMismatch[] {
  const mismatches: ReplayHeaderMismatch[] = [];

  const check = (field: keyof ReplayValidationConfig, recorded: number, current: number, tol = 1e-6) => {
    const absDiff = Math.abs(recorded - current);
    if (absDiff > tol) {
      mismatches.push({ field, recorded, current, absDiff });
    }
  };
  const checkBool = (field: keyof ReplayValidationConfig, recorded: boolean, current: boolean) => {
    if (recorded !== current) {
      mismatches.push({ field, recorded, current });
    }
  };
  const checkVec3 = (
    field: 'extVelAirDamping' | 'extVelGroundDamping',
    recorded: [number, number, number],
    current: [number, number, number],
    tol = 1e-6
  ) => {
    const absDiff = Math.max(
      Math.abs(recorded[0] - current[0]),
      Math.abs(recorded[1] - current[1]),
      Math.abs(recorded[2] - current[2])
    );
    if (absDiff > tol) {
      mismatches.push({ field, recorded: recorded.join(','), current: current.join(','), absDiff });
    }
  };

  check('tickRateHz', header.tickRateHz, config.tickRateHz);
  check('gravity', header.gravity, config.gravity);
  check('jumpSpeed', header.jumpSpeed, config.jumpSpeed);
  check('moveSpeedGround', header.moveSpeedGround, config.moveSpeedGround);
  check('moveSpeedAir', header.moveSpeedAir, config.moveSpeedAir);
  check('colliderHeight', header.colliderHeight, config.colliderHeight);
  check('colliderRadius', header.colliderRadius, config.colliderRadius);
  checkVec3('extVelAirDamping', header.extVelAirDamping, config.extVelAirDamping);
  checkVec3('extVelGroundDamping', header.extVelGroundDamping, config.extVelGroundDamping);
  check('gravityShapeRiseMult', header.gravityShapeRiseMult, config.gravityShapeRiseMult);
  check('gravityShapeApexMult', header.gravityShapeApexMult, config.gravityShapeApexMult);
  check('gravityShapeFallMult', header.gravityShapeFallMult, config.gravityShapeFallMult);
  check('gravityShapeApexThreshold', header.gravityShapeApexThreshold, config.gravityShapeApexThreshold);
  check('gravityShapeKneeWidth', header.gravityShapeKneeWidth, config.gravityShapeKneeWidth);
  checkBool('gravityShapeOnlyJumps', header.gravityShapeOnlyJumps, config.gravityShapeOnlyJumps);
  check('stepHeight', header.stepHeight, config.stepHeight);
  check('terminalVelocity', header.terminalVelocity, config.terminalVelocity);
  check('maxSlopeRadians', header.maxSlopeRadians, config.maxSlopeRadians);
  check('maxPenetrationDepth', header.maxPenetrationDepth, config.maxPenetrationDepth);
  check('coyoteTimeSeconds', header.coyoteTimeSeconds, config.coyoteTimeSeconds);
  check('minJumpDelaySeconds', header.minJumpDelaySeconds, config.minJumpDelaySeconds);
  checkBool('easyModeMovement', header.easyModeMovement, config.easyModeMovement);
  check('colliderShape', header.colliderShape, config.colliderShape);
  checkBool('dashEnabled', header.dashEnabled, config.dashEnabled);
  check('dashMagnitude', header.dashMagnitude, config.dashMagnitude);
  check('minDashDelaySeconds', header.minDashDelaySeconds, config.minDashDelaySeconds);
  checkBool('dashUseExternalVelocity', header.dashUseExternalVelocity, config.dashUseExternalVelocity);

  return mismatches;
}

export async function fetchReplayForPlay(playId: string): Promise<Uint8Array | null> {
  const res = await fetch(`/api/plays/${encodeURIComponent(playId)}/replay`, { credentials: 'include' });
  if (!res.ok) return null;
  return new Uint8Array(await res.arrayBuffer());
}
