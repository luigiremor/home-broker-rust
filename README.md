# Home Broker em Rust

## Descrição

Este projeto é uma simulação de um Home Broker (plataforma de negociação online) com um motor de correspondência de ordens (matching engine), implementado em Rust. Ele apresenta uma interface de usuário baseada em terminal (TUI) para visualizar ordens de compra (bids), ordens de venda (asks), negócios realizados e o preço médio atual. O sistema gera continuamente ordens aleatórias e as processa através do motor de correspondência.

## Funcionalidades

- **Gerenciamento do Livro de Ofertas**: Mantém listas separadas para ordens de compra (bids) e ordens de venda (asks).
- **Motor de Correspondência (Matching Engine)**: Corresponde ordens de compra e venda com base na prioridade preço-tempo.
- **Atualizações em Tempo Real**: A TUI é atualizada em tempo real para refletir as mudanças no livro de ofertas e novos negócios.
- **Interface de Usuário no Terminal (TUI)**: Fornece uma representação visual de:
  - Ordens de Compra (Bids)
  - Ordens de Venda (Asks)
  - Negócios Realizados
  - Preço Médio
- **Processamento Concorrente de Ordens**: Utiliza um pool de threads para submeter ordens concorrentemente.
- **Suporte a Docker**: Pode ser construído e executado como um contêiner Docker.

## Pré-requisitos

Para executar este projeto, você precisará de:

- **Rust**: Para compilar e executar o projeto localmente. (Instale em [rust-lang.org](https://www.rust-lang.org/tools/install))

  - Instale o Rust:

  ```bash
    curl https://sh.rustup.rs -sSf | sh
  ```

- **Docker**: Para construir e executar o projeto em um contêiner. (Instale em [docker.com](https://www.docker.com/get-started))

## Como Executar

### Localmente

1.  **Execute a aplicação:**
    ```bash
    cargo run
    ```
    Isso irá compilar e iniciar a aplicação, exibindo a TUI no seu terminal.

### Usando Docker

1.  **Construa a imagem Docker:**
    A partir do diretório raiz do projeto (onde o `Dockerfile` está localizado):

    ```bash
    docker build -t home-broker .
    ```

2.  **Execute o contêiner Docker:**
    ```bash
    docker run --rm -it home-broker
    ```
    As flags `-it` executam o contêiner em modo interativo com um TTY, e `--rm` remove o contêiner quando ele é finalizado.

## Controles

- **Sair**: Pressione `q` ou `Ctrl+C` para sair da aplicação quando a TUI estiver ativa.

## Estrutura do Projeto (Visão Geral)

- `src/main.rs`: Ponto de entrada da aplicação, inicializa a TUI, o livro de ofertas e a geração de ordens.
- `src/core/`: Contém a lógica principal:
  - `orderbook.rs`: Implementação do livro de ofertas e do motor de correspondência.
  - `errors.rs`: Define tipos de erro personalizados.
- `src/models/`: Define as estruturas de dados:
  - `order.rs`: Representa uma ordem de compra ou venda.
  - `trade.rs`: Representa um negócio realizado.
  - `side.rs`: Enum para o lado da ordem (Compra/Venda).
- `src/ui/`:
  - `tui.rs`: Gerencia a renderização da interface de usuário no terminal usando `ratatui`.
- `src/utils/`:
  - `random.rs`: Utilitário para gerar ordens aleatórias.
  - `sync/`: Primitivas de sincronização personalizadas (se houver, com base nas importações).
- `src/sync/`:
  - `mpsc.rs`: Implementação de um canal de mensagens concorrente.
  - `rwlock.rs`: Implementação de um lock de leitura/escrita.
  - `threadpool.rs`: Implementação de um pool de threads.
- `Dockerfile`: Instruções para construir a imagem Docker.
- `Cargo.toml`: Manifesto do projeto que define dependências e metadados.

## Participantes:

- Luigi Remor Costa (23203395)
- Lucas Nunes Bossle (23100751)
- Gustavo Luiz Kohler (23102480)
