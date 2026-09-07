import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import type { Viz } from 'src/viz';
import { buildCustomShader } from 'src/viz/shaders/customShader';

interface RobotCharacterConf {
  /** Full collider height (feet to top); the model is uniformly scaled to match it. */
  colliderHeight: number;
  centerToFeetOffset: number;
  /** Floor on the sped-up walk cycle so the swing phase still spans a few frames. */
  minWalkCycleSeconds: number;
}

const WalkFadeTau = 0.08;
const YawTau = 0.06;
const MinWalkSpeed = 0.5;

const expDecay = (dt: number, tau: number) => 1 - Math.exp(-dt / tau);
const wrapAngle = (a: number) => Math.atan2(Math.sin(a), Math.cos(a));

/**
 * Body speed at which the planted feet don't slide, in model units/s at 1x: per foot, stride
 * along the walk axis (model -X) divided by time spent near its lowest point.  Feet are leaf
 * bones in the lower part of the model, extended along their axis to the floor.  Leaves the
 * mixer at t=0; NaN when no feet are found.
 */
const estimateWalkGroundSpeed = (
  model: THREE.Object3D,
  clip: THREE.AnimationClip,
  mixer: THREE.AnimationMixer
): number => {
  model.updateMatrixWorld(true);
  const bbox = new THREE.Box3().setFromObject(model);
  const height = bbox.max.y - bbox.min.y;
  const feet: { bone: THREE.Bone; tipLen: number }[] = [];
  const pos = new THREE.Vector3();
  const dir = new THREE.Vector3();
  model.traverse(obj => {
    if (!(obj instanceof THREE.Bone) || obj.children.length > 0) {
      return;
    }
    obj.getWorldPosition(pos);
    dir.set(0, 1, 0).transformDirection(obj.matrixWorld);
    if (pos.y < bbox.min.y + 0.4 * height && dir.y < -0.2) {
      feet.push({ bone: obj, tipLen: (bbox.min.y - pos.y) / dir.y });
    }
  });

  const N = 120;
  const tracks = feet.map(() => [] as THREE.Vector3[]);
  for (let i = 0; i < N; i++) {
    mixer.setTime((i / N) * clip.duration);
    model.updateMatrixWorld(true);
    feet.forEach(({ bone, tipLen }, k) => tracks[k].push(bone.localToWorld(new THREE.Vector3(0, tipLen, 0))));
  }
  mixer.setTime(0);

  const speeds = tracks
    .map((pts, k) => {
      const xs = pts.map(p => p.x);
      const ys = pts.map(p => p.y);
      const minY = Math.min(...ys);
      const lift = Math.max(...ys) - minY;
      const stanceFrac = ys.filter(y => y < minY + 0.25 * lift).length / N;
      const stride = Math.max(...xs) - Math.min(...xs);
      console.info(
        `[robotCharacter] foot ${feet[k].bone.name}: stride ${stride.toFixed(2)} lift ${lift.toFixed(2)} stance ${(stanceFrac * 100).toFixed(0)}%`
      );
      return stride / (stanceFrac * clip.duration);
    })
    .sort((a, b) => a - b);
  return speeds[speeds.length >> 1];
};

/**
 * Prototype skinned player model.  Returns the root group to pass as `player.mesh`; physics
 * copies the player position into it every frame and this drives yaw, walk playback, and
 * first-person hiding on top.  The glb faces -X; `pivot` re-aims it to +Z so root yaw is
 * `atan2(dir.x, dir.z)`.
 */
export const buildRobotCharacter = (viz: Viz, conf: RobotCharacterConf): THREE.Group => {
  const root = new THREE.Group();
  const pivot = new THREE.Group();
  pivot.rotation.y = Math.PI / 2;
  root.add(pivot);

  const material = buildCustomShader(
    { color: new THREE.Color(0xaaadb3), metalness: 0.75, roughness: 0.45 },
    {},
    { noOcclusion: true }
  );

  let mixer: THREE.AnimationMixer | null = null;
  let walk: THREE.AnimationAction | null = null;
  let walkWeight = 0;
  let worldGroundSpeed = NaN;
  let maxWalkTimeScale = 1;
  let yaw: number | null = null;

  new GLTFLoader().load(
    '/robot-character.glb',
    gltf => {
      const model = gltf.scene;
      model.traverse(obj => {
        if (obj instanceof THREE.Mesh) {
          obj.material = material;
          obj.castShadow = false;
          obj.receiveShadow = true;
          obj.frustumCulled = false;
        }
      });

      const bbox = new THREE.Box3().setFromObject(model);
      const scale = conf.colliderHeight / (bbox.max.y - bbox.min.y);
      pivot.scale.setScalar(scale);
      pivot.position.y = -conf.centerToFeetOffset - bbox.min.y * scale;

      const clip = gltf.animations.find(c => c.name === 'walk') ?? gltf.animations[0];
      mixer = new THREE.AnimationMixer(model);
      walk = mixer.clipAction(clip).play();
      worldGroundSpeed = estimateWalkGroundSpeed(model, clip, mixer) * scale;
      walk.setEffectiveWeight(0);
      maxWalkTimeScale = clip.duration / conf.minWalkCycleSeconds;
      console.info(
        `[robotCharacter] walk clip ${clip.duration.toFixed(2)}s: no-slide body speed ${worldGroundSpeed.toFixed(2)} u/s; playback capped at ${maxWalkTimeScale.toFixed(1)}x`
      );
      pivot.add(model);
    },
    undefined,
    err => console.error('Failed to load robot character', err)
  );

  viz.registerOnRespawnCb(() => {
    yaw = null;
  });

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    viz.registerBeforeRenderCb((_curTimeSeconds, dt) => {
      const isFirstPerson = viz.cameraController?.isFirstPerson ?? false;
      root.visible = !isFirstPerson;
      if (isFirstPerson) {
        return;
      }

      const [wx, , wz] = fpCtx.playerStateGetters.getWalkVelocity();
      const speed = Math.hypot(wx, wz);
      const onGround = fpCtx.playerStateGetters.getIsOnGround();

      if (yaw === null) {
        yaw = (viz.cameraController?.angles.theta ?? 0) + Math.PI;
      }
      if (speed > MinWalkSpeed) {
        yaw += wrapAngle(Math.atan2(wx, wz) - yaw) * expDecay(dt, YawTau);
      }
      root.rotation.y = yaw;

      if (!mixer || !walk) {
        return;
      }
      const targetWeight = onGround && speed > MinWalkSpeed ? 1 : 0;
      walkWeight += (targetWeight - walkWeight) * expDecay(dt, WalkFadeTau);
      walk.setEffectiveWeight(walkWeight);
      if (targetWeight > 0) {
        walk.setEffectiveTimeScale(
          worldGroundSpeed > 0 ? Math.min(speed / worldGroundSpeed, maxWalkTimeScale) : 1
        );
      }
      mixer.update(dt);
    }, 2);
  });

  return root;
};
