# rlog-agent

**분산 시스템을 위한 경량 로그 수집 에이전트**

![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange?logo=rust)
![Tokio](https://img.shields.io/badge/Tokio-1.49-blue)
![gRPC](https://img.shields.io/badge/gRPC-Tonic_0.14-green)
![Protocol Buffers](https://img.shields.io/badge/Protocol_Buffers-Prost-yellow)

---

## 프로젝트 소개

rlog-agent는 분산 환경에서 로그를 중앙 서버로 효율적으로 수집하기 위해 설계된 경량 에이전트입니다. Rust의 성능과 안전성을 기반으로 비동기 I/O와 gRPC 스트리밍을 활용하여 대용량 로그를 안정적으로 전송합니다.

### 주요 특징

- **실시간 로그 수집**: 파일 시스템 이벤트 기반 감시로 즉각적인 로그 감지
- **배치 전송 최적화**: 크기 또는 시간 기반 플러시로 네트워크 효율성 극대화
- **자동 인증 관리**: Access/Refresh 토큰 자동 갱신 및 재등록
- **헬스 체크**: 주기적 Heartbeat로 에이전트 상태 모니터링
- **안정적 종료**: CancellationToken 기반 Graceful Shutdown으로 데이터 손실 방지

---

## 아키텍처

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              rlog-agent                                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐             │
│  │ Collector #1 │────→│              │     │              │             │
│  └──────────────┘     │              │     │              │             │
│  ┌──────────────┐     │   Forwarder  │────→│   Streamer   │────→  gRPC  │
│  │ Collector #2 │────→│   (Batcher)  │     │  (Streaming) │      Server │
│  └──────────────┘     │              │     │              │             │
│  ┌──────────────┐     │              │     │              │             │
│  │ Collector #N │────→│              │     │              │             │
│  └──────────────┘     └──────────────┘     └──────────────┘             │
│         │                                          │                     │
│     [LogEvent]                                [LogBatch]                 │
│                                                                          │
│  ┌──────────────┐                                   │                    │
│  │HealthReporter│───────────────────────────────────┼────→  gRPC Server │
│  └──────────────┘                                   │                    │
│                                                     │                    │
│  ┌──────────────┐     ┌──────────────┐              │                    │
│  │ TokenManager │←───→│AuthInterceptor│←────────────┘                    │
│  └──────────────┘     └──────────────┘                                   │
│         │                                                                │
│         └──────────────────────────────────────────────────→ AuthService │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 컴포넌트 설명

| 컴포넌트 | 역할 |
|----------|------|
| **Collector** | 파일 변경 감지 및 로그 라인 추출 (다중 인스턴스 지원) |
| **Forwarder** | LogEvent를 배치로 묶어 LogBatch 생성 |
| **Streamer** | gRPC 클라이언트 스트리밍으로 서버에 전송 |
| **HealthReporter** | 주기적 Heartbeat 전송 (CPU/메모리 사용률) |
| **TokenManager** | Access/Refresh 토큰 관리 및 자동 갱신 |
| **AuthInterceptor** | gRPC 요청에 인증 헤더 자동 주입 |

---

## 기술 스택

| 기술 | 버전 | 용도 |
|------|------|------|
| **Rust** | 2024 Edition | 시스템 프로그래밍 언어 |
| **Tokio** | 1.49 | 비동기 런타임 |
| **Tonic** | 0.14 | gRPC 프레임워크 |
| **Prost** | 0.14 | Protocol Buffers 코드 생성 |
| **Notify** | 8.2 | 크로스 플랫폼 파일 감시 |
| **Tracing** | 0.1 | 구조화된 로깅 |
| **Sysinfo** | 0.35 | 시스템 리소스 모니터링 |
| **Chrono** | 0.4 | 시간/날짜 처리 |

---

## 주요 기능 상세

### 1. 파일 감시 기반 로그 수집

- `notify` 크레이트를 활용한 이벤트 기반 파일 감시
- 파일 로테이션 및 트런케이션 자동 감지
- 크로스 플랫폼 파일 식별:
  - Unix: `inode` 기반 식별
  - Windows: `creation_time` 기반 식별

```rust
#[cfg(unix)]
fn get_file_id(meta: &Metadata) -> u64 {
    meta.ino()
}

#[cfg(windows)]
fn get_file_id(meta: &Metadata) -> u64 {
    meta.creation_time()
}
```

### 2. 배치 처리 및 전송 최적화

- `batch_size` 도달 시 즉시 플러시 (기본값: 1000)
- `flush_interval` 주기로 강제 플러시 (기본값: 10초)
- 메모리 효율적인 배치 스왑 (`std::mem::take`)

```rust
tokio::select! {
    msg = self.rx.recv() => {
        // 배치 크기 도달 시 플러시
        if logs.len() >= self.batch_size {
            self.flush(&mut logs).await;
        }
    }
    _ = interval.tick() => {
        // 주기적 플러시
        self.flush(&mut logs).await;
    }
}
```

### 3. gRPC 스트리밍 통신

- **클라이언트 스트리밍 RPC**: 다수의 LogBatch를 하나의 연결로 전송
- Protocol Buffers 기반 효율적인 직렬화
- 인터셉터 패턴으로 투명한 인증 처리

### 4. 토큰 기반 인증

- **이중 토큰 체계**: Access Token + Refresh Token
- 401 Unauthenticated 응답 시 자동 토큰 갱신
- 갱신 실패 시 저장된 `agent_uuid`로 재등록
- 토큰 파일 권한 관리 (Unix: 0600)

```rust
match self.send_batch(batch).await {
    Ok(_) => Ok(()),
    Err(status) if status.code() == Code::Unauthenticated => {
        // 토큰 갱신 후 재시도
        self.token_manager.write().await.refresh().await?;
        self.send_batch(batch).await?;
        Ok(())
    }
    Err(e) => Err(e.into()),
}
```

### 5. 헬스 체크

- 10초 주기 Heartbeat 전송
- CPU 사용률 및 메모리 사용률 리포팅
- `sysinfo` 크레이트로 시스템 메트릭 수집

### 6. Graceful Shutdown

- `CancellationToken` 기반 종료 신호 전파
- `Ctrl+C` 시그널 감지
- Collector 종료 후 잔여 로그 플러시

```rust
tokio::select! {
    _ = signal::ctrl_c() => {
        info!("Ctrl+C 감지..");
    }
}
shutdown.cancel();  // 모든 컴포넌트에 종료 신호 전파
```

---

## 프로젝트 구조

```
rlog-agent/
├── src/
│   ├── main.rs              # 진입점, 컴포넌트 조율
│   ├── collector.rs         # 파일 감시 및 로그 수집
│   ├── forwarder.rs         # 배치 처리
│   ├── streamer.rs          # gRPC 스트리밍 전송
│   ├── health.rs            # 헬스 체크 리포터
│   ├── settings.rs          # 설정 관리 (YAML)
│   ├── models.rs            # 내부 데이터 모델
│   ├── proto.rs             # Proto 모듈 선언
│   └── auth/
│       ├── mod.rs           # 인증 모듈
│       ├── client.rs        # AuthService gRPC 클라이언트
│       ├── token_manager.rs # 토큰 저장/갱신 관리
│       └── interceptor.rs   # gRPC 인터셉터
├── proto/
│   ├── log.proto            # LogService 정의
│   ├── auth.proto           # AuthService 정의
│   └── health.proto         # HealthService 정의
├── config/
│   └── agent.yaml           # 런타임 설정 파일
├── state/
│   ├── token                # Refresh Token 저장
│   └── agent_uuid           # 에이전트 고유 식별자
├── Cargo.toml
└── build.rs                 # Proto 컴파일 스크립트
```

---

## 설치 및 실행

### 사전 요구사항

- Rust 1.85+ (2024 Edition)
- Protocol Buffers 컴파일러 (`protoc`)

### 빌드

```bash
# 릴리즈 빌드
cargo build --release
```

### 최초 실행 (에이전트 등록)

최초 실행 시 환경 변수를 통해 서버 정보를 전달합니다.

```bash
# Windows
set SERVER_ADDR=http://localhost:50051
set PROJECT_KEY=your-project-key
.\target\release\rlog-agent.exe

# Linux/macOS
SERVER_ADDR=http://localhost:50051 \
PROJECT_KEY=your-project-key \
./target/release/rlog-agent
```

### 이후 실행

등록 완료 후 `config/agent.yaml`이 자동 생성되며, 이후에는 환경 변수 없이 실행 가능합니다.

```bash
./target/release/rlog-agent
```

---

## 설정 파일

### config/agent.yaml

```yaml
server_addr: "http://localhost:50051"
project_key: "your-project-key"
batch_size: 1000          # 배치당 최대 로그 수
flush_interval: 10        # 플러시 주기 (초)
heartbeat_interval: 30    # 헬스체크 주기 (초)
sources:
  - label: "app"          # 로그 라벨 (식별용)
    path: "/var/log/app.log"
  - label: "error"
    path: "/var/log/error.log"
```

| 필드 | 타입 | 기본값 | 설명 |
|------|------|--------|------|
| `server_addr` | String | - | gRPC 서버 주소 |
| `project_key` | String | - | 프로젝트 식별 키 |
| `batch_size` | Integer | 1000 | 배치당 최대 로그 수 |
| `flush_interval` | Integer | 10 | 강제 플러시 주기 (초) |
| `heartbeat_interval` | Integer | 30 | 헬스체크 주기 (초) |
| `sources` | Array | - | 수집 대상 로그 파일 목록 |

---

## 기술적 하이라이트

### Rust 언어 활용

- **동시성 제어**: `Arc<RwLock<T>>`를 활용한 스레드 안전한 토큰 공유
- **조건부 컴파일**: `#[cfg(unix)]` / `#[cfg(windows)]`로 플랫폼별 최적화
- **소유권 시스템**: 컴파일 타임 메모리 안전성 보장

### 비동기 프로그래밍

- **tokio::select!**: 다중 비동기 이벤트 동시 처리
- **mpsc::channel**: 컴포넌트 간 비동기 메시지 전달
- **CancellationToken**: 계층적 종료 신호 전파

### gRPC 통신

- **Tonic 스트리밍**: 클라이언트 스트리밍 RPC 구현
- **Interceptor 패턴**: 요청 전처리로 인증 헤더 자동 주입
- **에러 처리**: gRPC Status 코드 기반 재시도 로직

### 에러 처리

- **anyhow 크레이트**: Context 기반 에러 체이닝
- **Graceful 복구**: 인증 실패 시 자동 갱신/재등록

---

## gRPC 프로토콜 정의

### LogService

```protobuf
service LogService {
  // 클라이언트 스트리밍: 다수의 LogBatch 전송
  rpc Send(stream LogBatch) returns (google.protobuf.Empty);
}

message LogBatch {
  string batch_id = 1;
  google.protobuf.Timestamp send_at = 2;
  repeated Log logs = 3;
}

message Log {
  string label = 1;
  string line = 2;
  google.protobuf.Timestamp timestamp = 3;
}
```

### AuthService

```protobuf
service AuthService {
  // 에이전트 등록 (최초 또는 재등록)
  rpc Register(RegisterRequest) returns (RegisterResponse);

  // Access Token 갱신
  rpc Refresh(RefreshRequest) returns (RefreshResponse);
}
```

### HealthService

```protobuf
service HealthService {
  // 주기적 상태 리포팅
  rpc Heartbeat(HeartbeatRequest) returns (google.protobuf.Empty);
}

message HeartbeatRequest {
  google.protobuf.Timestamp timestamp = 1;
  double cpu = 2;      // CPU 사용률 (%)
  double memory = 3;   // 메모리 사용률 (%)
}
```

---

## 라이선스

MIT License
