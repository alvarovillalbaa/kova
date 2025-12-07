- Building from Prototype to Test to Production
    
    Buenísima pregunta, porque aquí es donde muchos proyectos se quedan en PPT-chain.
    
    Voy a asumir dos posibles caminos (los dos valen para lo que estás imaginando):
    
    - **Nuevo L1 / appchain propia**
    - **Rollup / L2 modular encima de otra L1 (tipo Ethereum/Celestia)**
    
    Te marco un proceso general que sirve para ambos.
    
    ---
    
    ## A) ¿Cómo probarlo? (antes de que nadie más lo toque)
    
    ### 1. Base: especificación + tests de lógica
    
    **Objetivo:** que la *lógica del protocolo* esté bien antes de meter red, nodos, etc.
    
    1. **Especificación clara (aunque sea en Markdown):**
        - State machine: qué es el estado (cuentas, balances, contratos, etc.).
        - Transiciones: qué hace cada tipo de transacción, bloque, prueba ZK, etc.
        - Consenso: pasos exactos del protocolo (proponer, votar, finality, slashing…).
        - Fees, gas, límites de bloque, etc.
    2. **Unit tests + property-based tests:**
        - Criptografía: firmas, hashes, ZK proofs/verifiers → deben fallar cuando toca.
        - State transitions:
            - “Si A manda 10 a B, el balance baja 10 y sube 10.”
            - “Si falla la prueba, la tx / bloque se rechaza.”
        - Reglas de consenso:
            - Bloques inválidos no se aceptan.
            - Reorgs y forks se resuelven como esperas.
    
    > Herramientas típicas (según stack): testing framework de tu lenguaje + algo tipo property-based (QuickCheck/Hypothesis/fast-check).
    > 
    
    ---
    
    ### 2. Simulación de red y “devnet” local
    
    **Objetivo:** ver tu chain en modo multi-nodo, con forks, latencia y caos.
    
    1. **Devnet local:**
        - 3–5 nodos corriendo en tu máquina con Docker / docker-compose.
        - Fuerzas:
            - cortes de red (simular particiones),
            - nodos lentos,
            - nodos que envían bloques inválidos,
            - spam de transacciones.
    2. **Pruebas de consenso:**
        - ¿Todos convergen al mismo head después de una partición?
        - ¿Consigues finality (PoS+BFT) en los tiempos esperados?
        - ¿Qué pasa cuando un validador deja de firmar / se comporta mal?
    3. **Benchmarks básicos:**
        - TPS real que alcanzas en devnet.
        - Latencia entre envío de tx y inclusión en bloque.
        - Uso de CPU/RAM bajo carga.
    
    ---
    
    ### 3. Seguridad y criptografía
    
    **Objetivo:** que no haya agujeros obvios que te rompan la chain.
    
    1. **Revisión de código externa** (aunque sea freelance):
        - Módulos críticos: consenso, verificación de bloques, verificación ZK, fees.
    2. **Fuzzing / differential testing:**
        - Mandar inputs basura / aleatorios a la VM, al verificador, al parser de tx.
        - Si haces VM propia o EVM-like, comparar comportamiento con un cliente de referencia (donde tenga sentido).
    3. **Formal verification (si apuntas alto):**
        - Sobre todo para:
            - circuito/verificador ZK,
            - reglas de consenso BFT.
    
    No hace falta que todo esté 100% formalmente verificado, pero al menos las piezas que, si fallan, matan todo el sistema.
    
    ---
    
    ### 4. Testnet pública
    
    **Objetivo:** dejar que “desconocidos” la rompan.
    
    1. **Lanzar una testnet pública:**
        - Faucet para tokens de prueba.
        - Block explorer sencillo.
        - Docs mínimas para:
            - correr un nodo,
            - desplegar una dApp,
            - mandar txs.
    2. **Forzar upgrades y forks en testnet:**
        - Probar cómo haces:
            - upgrade de protocolo,
            - hard fork / soft fork,
            - cambios de parámetros (gas, bloques, validators).
    3. **Incentivized testnet (opcional pero potente):**
        - Retos: encontrar bugs, romper cosas, saturar la red.
        - Recompensas: tokens futuros / puntos.
    
    ---
    
    ## B) ¿Cómo pasarlo a producción? (para que la gente lo use de verdad)
    
    Aquí diverge un poco según seas **L1/appchain** o **rollup/L2**.
    
    ---
    
    ### 1. Si haces un L1 / appchain propia
    
    1. **Genesis & bootstrap:**
        - Definir:
            - `chain_id`,
            - lista inicial de validadores,
            - distribución de tokens,
            - parámetros (tiempo de bloque, gas, slashing, etc.).
        - Crear el **genesis file** y coordinar con los validadores que van a estar en el bloque 0.
    2. **Infra mínima en producción (managed por ti):**
        - 2–3 **seed nodes** y 1–2 nodos RPC públicos (full nodes).
        - Block explorer (Blockscout / similar).
        - Endpoint público para wallets, dApps, indexers.
    3. **Distribuir la validación:**
        - Abrir el set de validadores:
            - staking/discovery desde el principio,
            - docs para que terceros puedan levantar nodos.
        - Asegurarte de que no sois solo “tú y tu colega” validando todo.
    4. **Operación continua:**
        - Monitoring (Prometheus/Grafana, logs).
        - Alertas: finality stall, forks largos, caídas de nodos.
        - Plan de upgrades:
            - desarrollo → testnet → mainnet con ventanas claras y anuncio previo.
    
    ---
    
    ### 2. Si haces un rollup / L2 modular
    
    **Más realista para ti a corto plazo** y mucho mejor para tracción.
    
    1. **Contratos en L1 host (ej. Ethereum/Celestia):**
        - Deploy de:
            - contrato de **bridge** (depósitos / retiros),
            - contrato de **rollup** (estado del L2 + verificación de pruebas / pruebas de fraude),
            - si usas Validium/Volition: contrato para DA/roots.
    2. **Infra de L2:**
        - **Secuenciador**:
            - servicio que ordena txs, produce bloques L2, publica batches al L1.
        - **Prover** (ZK o fraud proof infra):
            - genera pruebas ZK o maneja pruebas de fraude.
        - **RPC L2** para dApps y wallets.
    3. **Onboarding de usuarios:**
        - Front-end para:
            - puente L1 ↔ L2,
            - ver el estado (explorer L2),
            - docs para desplegar dApps (si es generic rollup).
    4. **Seguridad / gobernanza inicial:**
        - Multi-sig / governance para:
            - upgrades de los contratos de rollup,
            - cambios en el secuenciador (ir de centralizado a descentralizado),
            - switches tipo rollup ↔ validium (si haces volition).
    
    ---
    
    ### 3. Cosas comunes a cualquier lanzamiento
    
    Da igual si eres L1, L2, appchain o DA layer, necesitas:
    
    1. **Versionado y releases:**
        - Semver.
        - CHANGELOG claro por release.
        - Binarios / imágenes de Docker oficiales.
    2. **Documentación mínima decente:**
        - Cómo correr un nodo.
        - Cómo conectarse (RPC, websockets, endpoints).
        - Cómo desplegar dApps (si aplica).
        - Ejemplos de scripts (CLI, SDK).
    3. **Ecosistema inicial:**
        - Explorer.
        - Alguna wallet integrada (aunque sea tu propia web wallet).
        - Alguna dApp de ejemplo (DEX simple, token, etc.) para mostrar que funciona.
    4. **Bug bounty público (aunque sea pequeño):**
        - Reportes coordinados.
        - Recompensas (tokens, dinero, reconocimiento).
    
    ---
    
    ## C) Para ti, en concreto (recorte brutal de camino)
    
    Dado lo que priorizas y tu contexto:
    
    - **Fase 1 – Prototipo serio:**
        - Implementar tu lógica como **rollup / appchain** encima de algo existente (OP Stack, Cosmos SDK, o un zk-rollup stack tipo zkSync/StarkNet si tienes músculo).
        - Tener:
            - test suite decente,
            - devnet local,
            - testnet pública.
    - **Fase 2 – Modularidad real:**
        - Separar **capa de ejecución** (VM, contratos) de:
            - capa de consenso/orden (secuenciador),
            - capa de DA (Ethereum vs Celestia vs tu propia).
        - Introducir “modos” tipo **Volition** si quieres DA configurable (on-chain/off-chain).
    - **Fase 3 – Mainnet controlada:**
        - Mainnet “beta” con:
            - pocos validadores/secuenciadores al inicio,
            - límites de gas agresivos,
            - upgrade powers bastante controlados (multi-sig técnica → luego governance).
    
    A partir de ahí, ya es iterar como cualquier SaaS, pero con la movida de que aquí un bug puede matar toda la historia.
    
    ---
    
    Pregunta para empujar esto a algo accionable:
    
    ¿Quieres que bajemos esto a un **roadmap de 6–12 meses** con milestones (MVP rollup, testnet, audits, mainnet beta) y tech stack sugerido (lenguajes, frameworks concretos) para que puedas mapearlo a tu realidad de tiempo/equipo?
    
- Interoperability & messaging
    
    **protocolo tipo IBC pero con opción de ZK-proofs**, o sea:
    
    - light-client clásico como baseline,
    - posibilidad de **sustituir algunas pruebas por ZK** para abaratar/verificar cosas complejas.
    
    **Light-client / proof-based (tipo IBC)**
    
    - Cada cadena mantiene un **light client** de la otra:
        - verifica headers usando firmas/compromisos de la cadena remota.
    - Seguridad **trust-minimized**: confías en el consenso de la chain remota, no en un multisig.
    
    **Relayers permissionless**
    
    - No es un modelo de seguridad, es un **rol**:
        - Cualquiera puede empujar pruebas/mensajes de una chain a otra.
    - En modelos light-client, relayer ≠ trusted: sólo transporta datos, la verificación la hace el contrato o módulo on-chain.
    
    **Mensajería cross-domain interna + externa**
    
    - **Interna:** dentro de tu propio ecosistema (domains/rollups/subnets).
    - **Externa:** hacia otras L1/L2 (Ethereum, Cosmos, etc.).
- PoS + BFT; Hotstuff-like
    
    
    | Protocolo | Frontier / innovación | Interoperabilidad | Privacidad ZK | Modularidad | Gobernanza descentralizada | Escalabilidad (L1+L2) | dApps |
    | --- | --- | --- | --- | --- | --- | --- | --- |
    | **PoW** | ≈ (ya muy explorado) | ≈ | ≈ | ≈ | ≈ | ✖ | ≈ |
    | **PoS** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
    | **DPoS** | ≈ | ≈ | ≈ | ≈ | ≈ (voto delegado) | ✅ (rendimiento L1) | ✅ |
    | **PoA** | ✖ | ✖ (normalmente permisionado) | ≈ | ✅ (fácil de cambiar) | ✖ (centralizado) | ✅ | ≈ |
    | **PoS + BFT** | ✅✅ | ✅✅ | ✅✅ | ✅ | ✅✅ | ✅✅ (ideal con rollups) | ✅✅ |
    | **DAG – Avalanche** | ✅✅ | ✅ | ✅ | ≈ | ≈ | ✅✅ | ✅ |
    | **DAG – Hashgraph** | ✅ | ≈ | ≈ | ≈ | ≈ | ✅ | ≈ |
    | **Proof of Space / Capacity** | ≈ | ✖ | ✖ | ≈ | ≈ | ✖ | ✖ |
    | **Proof of Replication** | ≈ | ✖ | ✖ | ≈ | ≈ | ✖ | ✖ |
    | **PoET** | ≈ | ✖ | ✖ | ≈ | ✖ (dependes de hardware propietario) | ✅ | ✖ |
    | **Hybrid PoW/PoS** | ≈ | ≈ | ≈ | ≈ | ≈ | ≈ | ≈ |
    | **Proof of Burn** | ✖ | ✖ | ✖ | ✖ | ✖ | ✖ | ✖ |
    | **Proof of Activity** | ≈ | ≈ | ≈ | ≈ | ≈ | ≈ | ≈ |
    | **Proof of Importance / Reputation** | ✅ (mucho campo de I+D) | ≈ | ≈ | ≈ | ✅ | ≈ | ≈ |
    | **Rollups + DAC (L2)** | ✅✅ | ✅✅ | ✅✅ (perfecto para zk-rollups) | ✅✅ | ✅ (on-chain governance + upgrades) | ✅✅✅ | ✅✅ |
    
    ### 2.1. Qué significa “PoS + BFT” en concreto
    
    Tres grandes familias que te interesan:
    
    1. **Tendermint / CometBFT-like**
        - BFT clásico, rondas de propuesta+votación, hasta 1/3 byzantinos.
        - Finalidad **inmediata** en cuanto hay 2/3 de votos en una ronda. [arXiv+1](https://arxiv.org/abs/1807.04938?utm_source=chatgpt.com)
    2. **HotStuff-like**
        - BFT moderno, leader-based, comunicación **lineal** en nº de nodos.
        - Diseñado para ser más simple y eficiente en cambios de líder y grandes conjuntos de validadores. [arXiv+1](https://arxiv.org/abs/1803.05069?utm_source=chatgpt.com)
    3. **Gasper / Casper-FFG + LMD-GHOST (Ethereum)**
        - Un **fork choice** tipo LMD-GHOST + un **finality gadget** (Casper FFG).
        - Finalidad no inmediata: finaliza por epochs, pensado para miles de validadores. [ethereum.org+1](https://ethereum.org/developers/docs/consensus-mechanisms/pos/gasper/?utm_source=chatgpt.com)
    
    ### 2.2. Tabla de decisión PoS+BFT
    
    (✅ fuerte, ⚠️ depende / medio, ❌ débil)
    
    ### Opciones principales
    
    | Opción | Finalidad | Latencia típica | Escala # validadores | Complejidad implementación | Ajuste con tu visión (rollups + domains + ZK) |
    | --- | --- | --- | --- | --- | --- |
    | **Tendermint-like** | ✅ Inmediata (1–2 rondas) | Baja–media | ⚠️ Bien hasta cientos, miles cuesta por O(N²) mensajes | ⚠️ Media (protocolo clásico BFT) | ✅ Muy buen core para L1 “cohesivo” + domains limitados |
    | **HotStuff-like** | ✅ Inmediata (3 fases pipeline) | Baja–media | ✅ Mejor para muchos validadores (O(N) view change) | ⚠️ Algo más complejo conceptualmente | ✅✅ Muy alineado con L1 PoS+BFT + muchos domains/rollups |
    | **Gasper-like (Ethereum)** | ⚠️ Finalidad diferida (epochs) | Media–alta | ✅✅ Miles de validadores | ✅ Probado en mainnet, pero más complejo de razonar | ⚠️ Bueno si copias ethos Ethereum, menos si quieres finality agresiva |
    
    Comentarios rápidos:
    
    - **Si quieres finality muy rápida** para ser la “L1 de rollups/domains”:
        
        → tiraría a **HotStuff-like** o **Tendermint-like** bien tuneado.
        
    - **Si quieres miles de validadores muy distribuidos** y te da igual tardar más en finalizar:
        
        → enfoque Gasper-like.
        
    
    ### 2.3. ¿Dónde entran las ZK-proofs en este cuadro?
    
    ZK no es “otro consenso”, es una **capa de verificación** que se puede usar en varios puntos:
    
    | Uso de ZK-proof | Qué verifica | Quién la comprueba | Efecto práctico |
    | --- | --- | --- | --- |
    | **Prueba de bloque L1** | Que el bloque respeta reglas de estado | Validadores BFT de L1 | Menos carga de ejecución por validador; si falla, bloque rechazado |
    | **Prueba de rollup** | Que el nuevo estado del rollup es válido | L1 (como contrato o módulo) | L1 no re-ejecuta el rollup, sólo verifica prueba (típico ZK-rollup) |
    | **Prueba de dominio/appchain externa** | Que la cabecera/estado de ese dominio es correcto | L1 o bridge manager | Permite **bridges “validity-based”** entre domains/chains |
    | **Prueba de consenso** (más exótico) | Que una cierta combinación de votos se produjo | L1 o clientes ligeros | Compresión de firmas/votos, menos tráfico / firmas agregadas |
    
    Para tu diseño:
    
    - Lo normal sería:
        - BFT PoS (Tendermint/HotStuff flavor) **ordena** bloques.
        - Cada bloque o batch de tx puede venir con:
            - pruebas ZK de ejecución de estado (core zk-VM),
            - pruebas ZK de rollups / domains.
    - La seguridad “quién decide qué bloque gana” sigue siendo BFT PoS.
    - La seguridad “si el contenido del bloque/rollup es válido” la da ZK.
- **Core = zk-VM / zk-EVM + STARKs ; Capa de red = soporte opcional de Mixnet**
    
    
    | Tecnología | Sin trusted setup | Cómputo general (arbitrary logic) | Oculta montos | Oculta remitente | Oculta receptor | Privacidad de red / metadatos | Post-quantum “amistosa” | Uso real en producción |
    | --- | --- | --- | --- | --- | --- | --- | --- | --- |
    | **ZK-SNARKs** (incl. PLONK/Halo2/etc.) | ⚠️ (clásicos no, muchas modernas sí/universal) | ✅ | ⚠️ (si las usas para eso) | ⚠️ | ⚠️ | ❌ | ❌ (curvas emparejadas) | ✅ |
    | **ZK-STARKs** | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ❌ | ✅ (hash-based) | ✅ (pero menos extendido que SNARKs) |
    | **zk-VMs / zk-EVMs** | ⚠️ (depende de la prueba subyacente) | ✅✅ (pensados para eso) | ⚠️ | ⚠️ | ⚠️ | ❌ | ⚠️ (según SNARK/STARK debajo) | ✅ (rollups, L2, etc.) |
    | **Bulletproofs** | ✅ | ⚠️ (range-proofs, no VM general) | ✅ (CT) | ❌ | ❌ | ❌ | ✅ (hash+curvas “normales”) | ✅ |
    | **Ring signatures** | ✅ | ❌ | ❌ | ✅ | ❌ | ❌ | ⚠️ (depende de esquema) | ✅ (Monero & co.) |
    | **Confidential Transactions (CT)** | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ | ⚠️ | ✅ (Monero, Mimblewimble, etc.) |
    | **Stealth addresses** | ✅ | ❌ | ❌ | ❌ | ✅ | ❌ | ✅ | ✅ |
    | **CoinJoin / PayJoin / CoinShuffle** | ✅ | ❌ | ❌ (montos visibles) | ✅ (difícil mapear entradas-salidas) | ⚠️ (según diseño) | ❌ | ✅ | ✅ (ecosistema Bitcoin) |
    | **Mimblewimble** | ✅ | ❌ (scriptless, muy limitado) | ✅ | ⚠️ (no hay direcciones “clásicas”) | ⚠️ | ❌ | ⚠️ | ✅ (Grin, Beam, LTC-EB) |
    | **Mixnets (Tor / Nym / etc.)** | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ (IP, timing, rutas) | ✅ | ✅ |
    | **Payment channels / Lightning-like** | ✅ | ❌ | ⚠️ (solo parte del flujo off-chain) | ⚠️ (topología oculta, pero hay fugas) | ⚠️ | ⚠️ | ✅ | ✅ |
    | **MPC** | ✅ (normalmente sin ceremonia tipo SNARK) | ✅ | ⚠️ | ⚠️ | ⚠️ | ❌ | ⚠️ (depende esquemas) | ✅ (use-cases puntuales) |
    | **FHE** | ✅ | ✅✅ (cómputo arbitrario cifrado) | ⚠️ | ⚠️ | ⚠️ | ❌ | ⚠️ | ⚠️ (poco práctico aún on-chain) |
    | **TEEs (SGX, etc.)** | ✅ (no hay “ceremonia”, pero confías en Intel/AMD) | ✅ | ⚠️ | ⚠️ | ⚠️ | ❌ | ❌ (hardware clásico) | ✅ (oráculos, validadores “confidenciales”) |
    
    | Tecnología | Pagos privados | dApps / cómputo privado | Rollups / escalado L2 | Identidad / credenciales | Privacidad red / metadatos |
    | --- | --- | --- | --- | --- | --- |
    | **ZK-SNARKs** | ✅ (si diseñas el protocolo) | ✅✅ | ✅✅ | ✅✅ | ❌ |
    | **ZK-STARKs** | ✅ (pagos escalables) | ✅ | ✅✅ | ✅ | ❌ |
    | **zk-VMs / zk-EVMs** | ✅ | ✅✅ | ✅✅✅ | ✅ | ❌ |
    | **Bulletproofs** | ✅✅ (montos ocultos) | ⚠️ | ⚠️ | ❌ | ❌ |
    | **Ring signatures** | ✅✅ (anonimizan remitente) | ❌ | ❌ | ⚠️ (identidad anónima) | ❌ |
    | **Confidential Transactions (CT)** | ✅✅ (core de pagos privados) | ❌ | ❌ | ❌ | ❌ |
    | **Stealth addresses** | ✅✅ (ocultan receptor) | ❌ | ❌ | ❌ | ❌ |
    | **CoinJoin / PayJoin / CoinShuffle** | ✅ (on-top de Bitcoin-like) | ❌ | ❌ | ❌ | ❌ |
    | **Mimblewimble** | ✅✅ | ❌ | ❌ | ❌ | ❌ |
    | **Mixnets (Tor / Nym / etc.)** | ⚠️ (oculta quién habla con quién) | ⚠️ | ⚠️ | ⚠️ | ✅✅ |
    | **Payment channels / Lightning-like** | ✅ (patrones de pago off-chain) | ❌ | ✅ (escalado de pagos) | ❌ | ⚠️ |
    | **MPC** | ⚠️ | ✅ (cálculo colaborativo sobre datos privados) | ⚠️ | ✅ | ❌ |
    | **FHE** | ⚠️ | ✅ (visión ideal de “smart contracts 100% cifrados”) | ⚠️ | ✅ | ❌ |
    | **TEEs (SGX, etc.)** | ⚠️ | ✅ (cómputo privado confiando en hardware) | ✅ (rollups/validadores “confidenciales”) | ✅ | ❌ |
- Combination: Optimistic Rollups + ZK-rollups + Payment/state channels para transacciones entre negocios con cierre mensual a L1
- Gobernanza descentralizada
- Arquitectura:
    - L1
        - Capa de Data Availability modular → Aumenta throughput usando **Data Availability Sampling**; throughput crece al añadir light nodes; permite bloques grandes y DA barata
        - Sharding
            - Nightshade — el estado y las transacciones se reparten en shards para paralelizar ejecución.
            - **Proto-danksharding (EIP-4844)**: ya desplegado, introduce *blobspace* para rollups.
            - **Danksharding completo**: muchas “data shards” para dar más capacidad a los rollups, pero la ejecución sigue centralizada en el beacon chain.
    - Rollups
        - **Shared sequencer** para múltiples rollups
        - **Shared sequencer + DA stack →** Secuenciador compartido para múltiples rollups; secuencia datos, los publica a DA y da soft+hard finality vía DA layer
        - Otros: NightSharding and DankSharding (Ethereum)
    - Subnets — para que otros puedan establecer su propia seguridad; Parachains — para que otros puedan establecer su propio estado y lógica; Appchains (para dApps) (ej: protocolo de interoperabilidad IBC en Cosmos)
        
        ```tsx
        
        Subnets — “para que otros establezcan su propia seguridad”
        
        Parachains — “para que otros establezcan su propio estado y lógica”
        
        Appchains — “para dApps”
        
        Problema:
        
        En la práctica, estos conceptos ya se pisan:
        
        Una appchain suele ya tener su propio estado y lógica.
        
        Una parachain es, de facto, una appchain con seguridad compartida con la relay chain.
        
        Una subnet puede usarse como appchain o como shard de un ecosistema.
        
        💡 Mejora:
        
        En lugar de 3 conceptos, define uno genérico tipo:
        
        “Domains” (dominios / zonas) con parámetros:
        
        modelo de seguridad: shared-security, sovereign, own-validator-set
        
        propósito: general-purpose, app-specific
        
        ubicación: on L1, rollup, subnet-like
        
        Así puedes decir en el spec:
        
        “El sistema soporta domains:
        
        Shared-security domains (tipo parachain)
        
        Sovereign domains (tipo appchain independiente que solo usa tu DA)
        
        Security-isolated domains (tipo subnet con su propio set de validadores)”
        
        Mismo concepto, distintas configuraciones. Menos ruido, más claridad.
        
        1. Domains vs escalabilidad
        
        TL;DR: Llamarlo “domains” no te limita la escalabilidad; lo que manda es:
        
        el bottleneck de disponibilidad de datos (DA),
        
        el cuello de botella del secuenciador,
        
        y cómo de caro es el cross-domain messaging.
        
        La abstracción “domain” es solo una forma de ordenar todo esto:
        
        un domain puede ser:
        
        una appchain soberana,
        
        una parachain con seguridad compartida,
        
        una subnet con su propio set de validadores,
        
        un rollup concreto (ZK/optimistic),
        
        incluso un L3 encima de un rollup.
        
        ¿Pierdes escalabilidad por meter una appchain dentro de una subnet?
        
        No por definición. Lo que ocurre es:
        
        Más niveles → más hops de mensajes:
        
        tx local dentro del dominio → rápida.
        
        tx que salta de un dominio a otro → tienes al menos:
        
        dominio origen → DA / L1,
        
        dominio destino.
        
        Shared sequencer único para todo puede convertirse en cuello de botella:
        
        si quieres máxima escalabilidad, probablemente acabes con:
        
        varios secuenciadores (por clases de domains),
        
        o un árbol de secuenciadores.
        
        Mi opinión (opinion):
        
        Diseñar todo en torno a “domains” parametrizables ayuda a escalar, porque:
        
        puedes aislar ruido (un juego loco no rompe el DEX),
        
        puedes escalar horizontalmente añadiendo más dominios.
        
        Para perseguir máxima escalabilidad:
        
        L1/DA muy fuerte + ZK-rollups de primera clase.
        
        Domains = vista lógica:
        
        unos domains son rollups con shared security,
        
        otros domains son subnets soberanas que sólo usan tu DA,
        
        otros domains son L3 appchains de nicho.
        
        Conclusión: usar “domains” como modelo mental no te resta TPS, sólo obliga a pensar muy bien:
        
        cómo compartes DA,
        
        cómo diseñas el/los sequencer(s),
        
        y cómo estructuras cross-domain messaging.
        ```
        
- Economic layer
    - Single Native Token — Domains pueden tener tokens propios, pero L1, DA, sequencer y mixnet **cobran siempre en X.**
    - ¿Quién cobra qué fees? (L1, rollups, domains, DA layer, sequencer compartido…)
        
        ### 1.3. Tabla actor → ingresos → costes → incentivos
        
        | Actor | Ingresos en X | Costes / riesgos | Diseño de incentivos recomendado |
        | --- | --- | --- | --- |
        | **Validador L1** | - Gas L1 (ejecución, verif. ZK)  - Parte de DA fees  - Emisión (si la hay) | - Stake bloqueado (slashing) - Hardware + ancho de banda | - Recompensas proporcionales a stake y performance. - Slashing fuerte por doble firma, censura deliberada, participación en bloques inválidos. |
        | **Nodos DA sampling** | - Porcentaje de DA fees  - Emisión específica DA (si quieres) | - Conectividad y almacenamiento a corto plazo | - Esquema tipo Celestia: light nodes prueban disponibilidad de datos → si DA falla, bloque no se considera válido.[Mitosis University+1](https://university.mitosis.org/understanding-data-availability-layers-celestia-eigenda/?utm_source=chatgpt.com) |
        | **Secuenciadores (shared)** | - Fees L2 (gas de rollups)  - MEV capturado (si lo permites)  - Posible “sequencer tips” | - Stake (slashable) - Riesgo de censura/boicot si abusan | - Requiere staking en X. - Slashing por equivocarse en reglas de ordenación/tiempos si violan protocolos; force-inclusion L1 como kill-switch si censuran.[The Flashbots Collective+1](https://collective.flashbots.net/t/the-economics-of-shared-sequencing/2514?utm_source=chatgpt.com) |
        | **Operadores de domains** | - Fees locales (gas del domain, en X o token local)  - Parte de MEV local | - Seguridad local (si own-security) - Gestión de su infra | - Si `shared-security domain`: seguridad viene de L1, pagan una *tarifa de seguridad* en X al L1. - Si `sovereign domain`: ellos mismos asumen seguridad; L1 cobra por DA y bridging. |
        | **Mixnet nodes** | - Fees por tráfico privado en X  - Emisión / rewards de privacidad | - Latencia, ancho de banda, stake (si añades) | - Modelo tipo NYM: stake + recompensas por performance, con usuarios pagando por ancho de banda, pero usando X en vez de un token separado.[Medium+1](https://medium.com/%40Dyacon_frost/nym-network-design-privacy-enhanced-access-and-token-incentives-9479a06f60e5?utm_source=chatgpt.com) |
        | **Relayers** | - Tips por mensaje entregado / cross-domain | - Operar infra | - Permissionless: cualquiera puede relayer; contrato solo paga si la prueba es válida. |
        
        ### 1.4. Flujo de fees recomendado (opinion)
        
        - **L1 gas**:
            - EIP-1559 style: base fee en X → una parte se **quema**, otra se distribuye a validadores.
        - **DA fees**:
            - Rollups/domains pagan X por byte publicado.
            - Distribución ejemplo: 70% validadores, 20% DA nodes, 10% treasury (I+D, grants).
        - **L2 fees (rollups)**:
            - Usuario paga gas en X (o token local convertido).
            - Secuenciador se queda parte, parte es reservada automáticamente para:
                - pagar DA en L1,
                - pagar una *security rent* al L1 si es shared-security-domain.
        - **Mixnet**:
            - Wallets que quieren privacidad de red pagan pequeños fees en X para enrutar tráfico.
            - Mixnodes cobran según volumen y calidad; reputación + staking.
- Seguridad por capas
    1. L1 (PoS+BFT HotStuff-like).
    2. DA layer (con sampling).
    3. Shared sequencer.
    4. Domains: `shared-security` vs `own-security`.
    5. Privacy infra (mixnet) —, que es más de “metadata security”, pero lo incluyo.
    
    ### 2.1. Matriz de seguridad por capa
    
    | Capa | Qué garantiza | Si se rompe… | Impacto | Mitigación / diseño recomendado |
    | --- | --- | --- | --- | --- |
    | **L1 (PoS+BFT HotStuff)** | - Orden global de bloques L1. - Correctitud de estado L1 (suponiendo ZK/validadores honestos). | Atacantes controlan ≥2/3 del stake. | Todo lo que ancle en L1 (rollups, domains con shared-security, bridges) puede ser reordenado o robado. | - Altísimo stake requerido. - Slashing agresivo. - Social recovery/fork como último recurso. |
    | **DA layer (on L1)** | - Que los datos de bloques/rollups publicados son accesibles (DA sampling). | Validadores intentan incluir bloques sin datos disponibles. | Clients no pueden reconstruir estado → deben rechazar esos bloques. | - Regla de consenso: bloque sin DA suficiente = inválido. - Clientes ligeros siguen solo cadena donde DA sampling pasa. |
    | **Shared sequencer** | - Ordenación de tx en múltiples rollups/domains. - No decide validez, solo orden. | Censura, reorder, MEV abusivo. No puede *validar* estados falsos si L1 verifica pruebas. | UX mala (censura), bricking temporal de rollups, MEV concentrada. No debería poder robar fondos por sí solo. | - `force-inclusion` en L1 para tx censuradas. - Rotación / comité de secuenciadores (PoS). - Opcionalmente múltiples sequencers en competencia.[Cube Exchange+1](https://www.cube.exchange/what-is/shared-sequencer?utm_source=chatgpt.com) |
    | **Domains shared-security** | - Domain corre su lógica, pero finality y seguridad vienen de L1. | Domain intenta producir estados inválidos. | Mientras L1 verifique pruebas (ZK o light-client), el daño queda acotado a censura o liveness; no puede engañar al L1. | - Requerir pruebas ZK/validity o light-client verificable en L1. - Pausar domain vía gobernanza si viola reglas. |
    | **Domains own-security (sovereign)** | - Seguridad solo depende de su propio consenso. | Validadores de ese domain coluden. | Los activos en ese domain y sus usuarios se ven comprometidos. L1 y otros domains no deberían romperse. | - Bridges con límites: cap de valor bridged, o “insured bridges”. - Tratarlos como *menos confiables* que shared-security. |
    | **Mixnet / privacy network** | - Ocultar metadatos (quién habla con quién, cuándo). | Nodo malicioso analiza tráfico. | Puede filtrar patrones de comunicación, pero no puede modificar estado on-chain. | - Diseño mixnet robusto (Nym-style).[Wikipedia+1](https://en.wikipedia.org/wiki/Nym_%28mixnet%29?utm_source=chatgpt.com) - Uso opcional para tráfico sensible. |
    
    ### 2.2. “Qué pasa si…”
    
    - **Shared sequencer se porta mal**:
        - Solo tiene poder sobre **orden y censura**, NO sobre validez.
        - Diseño clave:
            - Toda transacción tiene camino de *escape*:
                - usuario puede enviar tx directamente a L1 (con coste mayor) para forzar inclusión.
            - Métrica y slashing por censura sistemática/documentada.
    - **Un domain own-security cae (ataque 51% / byzantino)**:
        - Solo debe afectar:
            - a los assets que viven allí,
            - y a cualquier asset bridged que hayas permitido.
        - Por eso:
            - pones límites a cuánto valor puede quedar bloqueado en bridges hacia domains poco seguros,
            - usas modelos de “insurance / risk parameters” por domain.

- Privacy layer
    - **On-chain privacy**:
        - Pools blindados tipo Zcash,
        - ZK-circuits para:
            - montos ocultos,
            - direcciones ocultas (stealth addresses),
            - lógica privada para ciertos dApps.
    - **Network privacy**:
        - Mixnet como opción por defecto para:
            - clientes,
            - validadores,
            - sequencers.
- Dev Experience: Modular por plugins — para desarrollo continuado
    
    Clave: “plugins” suena a que cualquiera mete lógica en el core, y eso es peligroso. Hay que **estratificar**.
    
    ### 4.1. Niveles de plugins
    
    Piensa en 4 niveles:
    
    | Nivel | Nombre | Qué es | Quién puede cambiarlo | Riesgo | Ejemplo práctico |
    | --- | --- | --- | --- | --- | --- |
    | **0** | Kernel / Core protocol | Consenso, formato de bloque, DA rules, base VM. | Solo gobernanza L1, cambios raros. | Muy alto | HotStuff params, tamaño máximo de bloque, reglas de DA sampling. |
    | **1** | System modules / precompiles | Extensiones nativas de la VM (cripto avanzada, ZK helpers, syscalls). | Gobernanza L1 (on-chain upgrades). | Alto | Precompiles EVM para curvas, hash, precompiles ZK, bridges nativos.[Nervos Network+2Ethereum Stack Exchange+2](https://www.nervos.org/knowledge-base/what_are-precompiles_%28explainCKBot%29?utm_source=chatgpt.com) |
    | **2** | Domain templates | Tipos de domain: EVM-rollup, WASM-rollup, private-rollup, appchain template. | Gobernanza L1 (añadir tipos); domain owner instancia. | Medio | “EVM shared-security domain v1”, “private-Dex-domain v2”, etc. |
    | **3** | dApp plugins / contracts | Smart contracts, módulos de negocio. | Permissionless (con gas). | Bajo–medio | dApps normales, DeFi, NFT, etc. |
    
    ### 4.2. Cómo se desarrollan y despliegan
    
    ### SDKs
    
    - **Protocol SDK (nivel 0–1–2)**:
        - En Rust (o similar) para:
            - escribir system modules,
            - nuevas precompiles,
            - plantillas de domains.
        - Ciclo:
            - PR en repo core,
            - testnet,
            - votación on-chain para activarlo.
    - **dApp SDK (nivel 3)**:
        - Para EVM: Solidity, Vyper, etc.
        - Para WASM: Rust/AssemblyScript.
        - Para private domains: DSL específico para circuits/ZK.
    
    ### Versionado
    
    - Cada módulo de nivel 1–2 tiene:
        - `name`, `major.minor.patch`, `state-migration` opcional.
    - Principio:
        - **major**: cambio breaking (requiere migración y votación fuerte).
        - **minor/patch**: extensiones compatibles.
    
    ### Permisos / capabilities
    
    - Nivel 1–2:
        - Deben declarar **qué permisos tienen**:
            - ¿pueden tocar balances directamente?
            - ¿pueden leer todo el estado o solo su namespace?
            - ¿pueden emitir eventos cross-domain?
        - Gobernanza revisa la *capability manifest* antes de aprobar.
    - Nivel 3 (dApps):
        - Solo pueden usar lo que la VM expone (precompiles, syscalls).
        - No tienen permisos “privilegiados” fuera de su sandbox de contrato.
    
    ### 4.3. Combinación recomendada (opinion)
    
    > “Plugins” = principalmente System modules (precompiles) + Domain templates + dApps,
    > 
    > 
    > el core *no* es plug-and-play.
    > 
    
    Concretamente:
    
    1. **Core**:
        - Fijar HotStuff-like, DA sampling, zk-VM/STARKs como “no plugin”.
    2. **System modules**:
        - Añadir cripto avanzada vía precompiles:
            - curvas para ZK,
            - primitives para ring signatures, CT, FHE experiments.
    3. **Domain templates**:
        - Exponer 3–4 tipos iniciales:
            - `EVM-shared-security-domain`,
            - `ZK-privacy-domain`,
            - `Sovereign-appchain-domain`,
            - `Payment-channel-domain`.
    4. **dApps**:
        - Permitir a cualquiera desplegar sobre domains EVM/WASM.
        - Tooling fuerte: indexers, explorers, debuggers, simulación de ZK.