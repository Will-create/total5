use crate::Error;
use serde::Serialize;
#[cfg(feature = "postgres")]
use serde_json::Map;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct Data {
    root: PathBuf,
}

impl Data {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn insert<T: Serialize>(&self, collection: &str, model: T) -> Result<usize, Error> {
        let mut rows = self.load(collection).await?;
        rows.push(serde_json::to_value(model).map_err(|err| {
            Error::internal(format!("failed to serialize NoSQL document: {err}"))
        })?);
        self.save(collection, &rows).await?;
        Ok(1)
    }

    pub async fn find(&self, collection: &str) -> Result<Vec<Value>, Error> {
        self.load(collection).await
    }

    pub async fn read(&self, collection: &str, id: &str) -> Result<Option<Value>, Error> {
        Ok(self
            .load(collection)
            .await?
            .into_iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(id)))
    }

    pub async fn update<T: Serialize>(
        &self,
        collection: &str,
        id: &str,
        patch: T,
    ) -> Result<usize, Error> {
        let patch = serde_json::to_value(patch)
            .map_err(|err| Error::internal(format!("failed to serialize NoSQL patch: {err}")))?;
        let mut rows = self.load(collection).await?;
        let mut count = 0usize;
        for item in &mut rows {
            if item.get("id").and_then(Value::as_str) == Some(id) {
                if let (Some(target), Some(patch)) = (item.as_object_mut(), patch.as_object()) {
                    for (key, value) in patch {
                        target.insert(key.clone(), value.clone());
                    }
                    count += 1;
                }
            }
        }
        self.save(collection, &rows).await?;
        Ok(count)
    }

    pub async fn remove(&self, collection: &str, id: &str) -> Result<usize, Error> {
        let mut rows = self.load(collection).await?;
        let original = rows.len();
        rows.retain(|item| item.get("id").and_then(Value::as_str) != Some(id));
        let count = original.saturating_sub(rows.len());
        self.save(collection, &rows).await?;
        Ok(count)
    }

    async fn load(&self, collection: &str) -> Result<Vec<Value>, Error> {
        let path = self.collection_path(collection);
        match fs::read_to_string(&path).await {
            Ok(body) => serde_json::from_str(&body)
                .map_err(|err| Error::internal(format!("failed to parse NoSQL collection: {err}"))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(Error::internal(format!(
                "failed to read NoSQL collection: {err}"
            ))),
        }
    }

    async fn save(&self, collection: &str, rows: &[Value]) -> Result<(), Error> {
        let path = self.collection_path(collection);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|err| {
                Error::internal(format!("failed to prepare NoSQL directory: {err}"))
            })?;
        }
        let body = serde_json::to_vec_pretty(rows).map_err(|err| {
            Error::internal(format!("failed to serialize NoSQL collection: {err}"))
        })?;
        fs::write(path, body)
            .await
            .map_err(|err| Error::internal(format!("failed to write NoSQL collection: {err}")))
    }

    fn collection_path(&self, collection: &str) -> PathBuf {
        let safe = collection
            .trim_matches('/')
            .replace('\\', "_")
            .replace('/', "_");
        self.root.join(format!("{safe}.json"))
    }
}

#[derive(Debug, Clone)]
pub struct FileStorage {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredFile {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub content_type: String,
}

impl FileStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn save(
        &self,
        name: impl Into<String>,
        bytes: impl AsRef<[u8]>,
        content_type: impl Into<String>,
    ) -> Result<StoredFile, Error> {
        fs::create_dir_all(&self.root)
            .await
            .map_err(|err| Error::internal(format!("failed to prepare filestorage: {err}")))?;
        let name = name.into();
        let id = uuid::Uuid::new_v4().to_string();
        let path = self.root.join(&id);
        let mut file = fs::File::create(&path)
            .await
            .map_err(|err| Error::internal(format!("failed to create stored file: {err}")))?;
        file.write_all(bytes.as_ref())
            .await
            .map_err(|err| Error::internal(format!("failed to write stored file: {err}")))?;
        file.flush()
            .await
            .map_err(|err| Error::internal(format!("failed to flush stored file: {err}")))?;
        Ok(StoredFile {
            id,
            name,
            path,
            size: bytes.as_ref().len() as u64,
            content_type: content_type.into(),
        })
    }

    pub async fn read(&self, id: &str) -> Result<Vec<u8>, Error> {
        fs::read(self.root.join(id))
            .await
            .map_err(|err| Error::not_found(format!("stored file not found: {err}")))
    }

    pub async fn stat(&self, id: &str) -> Result<u64, Error> {
        fs::metadata(self.root.join(id))
            .await
            .map(|meta| meta.len())
            .map_err(|err| Error::not_found(format!("stored file not found: {err}")))
    }

    pub async fn remove(&self, id: &str) -> Result<(), Error> {
        fs::remove_file(self.root.join(id))
            .await
            .map_err(|err| Error::not_found(format!("stored file not found: {err}")))
    }
}

#[cfg(not(feature = "postgres"))]
pub struct PgDb {
    connection: String,
}

#[cfg(not(feature = "postgres"))]
impl PgDb {
    pub async fn connect(connection: &str) -> Result<Self, Error> {
        if connection.trim().is_empty() {
            return Err(Error::internal("missing Postgres connection string"));
        }
        Ok(Self {
            connection: connection.to_string(),
        })
    }

    pub fn connection(&self) -> &str {
        &self.connection
    }

    pub async fn query_json(&self, _sql: &str, _params: &[Value]) -> Result<Vec<Value>, Error> {
        Err(Error::internal(
            "Postgres execution backend is not enabled in this build",
        ))
    }
}

#[cfg(feature = "postgres")]
pub struct PgDb {
    client: tokio_postgres::Client,
}

#[cfg(feature = "postgres")]
impl PgDb {
    pub async fn connect(connection: &str) -> Result<Self, Error> {
        let (client, connection) = tokio_postgres::connect(connection, tokio_postgres::NoTls)
            .await
            .map_err(|err| Error::internal(format!("failed to connect to Postgres: {err}")))?;
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                tracing::error!("Postgres connection error: {err}");
            }
        });
        Ok(Self { client })
    }

    pub async fn query_json(&self, sql: &str, params: &[Value]) -> Result<Vec<Value>, Error> {
        let encoded = params.iter().map(Value::to_string).collect::<Vec<_>>();
        let refs = encoded
            .iter()
            .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let rows = self
            .client
            .query(sql, &refs)
            .await
            .map_err(|err| Error::internal(format!("Postgres query failed: {err}")))?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let mut obj = Map::new();
                for (index, column) in row.columns().iter().enumerate() {
                    obj.insert(column.name().to_string(), pg_cell_to_json(&row, index));
                }
                Value::Object(obj)
            })
            .collect())
    }
}

#[cfg(feature = "postgres")]
fn pg_cell_to_json(row: &tokio_postgres::Row, index: usize) -> Value {
    if let Ok(value) = row.try_get::<_, Option<String>>(index) {
        return value.map_or(Value::Null, Value::String);
    }
    if let Ok(value) = row.try_get::<_, Option<i64>>(index) {
        return value.map_or(Value::Null, |value| json!(value));
    }
    if let Ok(value) = row.try_get::<_, Option<i32>>(index) {
        return value.map_or(Value::Null, |value| json!(value));
    }
    if let Ok(value) = row.try_get::<_, Option<f64>>(index) {
        return value.map_or(Value::Null, |value| json!(value));
    }
    if let Ok(value) = row.try_get::<_, Option<bool>>(index) {
        return value.map_or(Value::Null, |value| json!(value));
    }
    Value::Null
}

#[derive(Debug, Clone, Default)]
pub struct Db;

impl Db {
    pub fn new() -> Self {
        Self
    }

    pub async fn connect(connection: &str) -> Result<PgDb, Error> {
        PgDb::connect(connection).await
    }

    pub fn find(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(QueryKind::Find, table)
    }

    pub fn list(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(QueryKind::List, table)
    }

    pub fn read(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(QueryKind::Read, table).limit(1)
    }

    pub fn one(&self, table: &str) -> QueryBuilder {
        self.read(table)
    }

    pub fn count(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(QueryKind::Count, table)
    }

    pub fn insert<T: Serialize>(&self, table: &str, model: T) -> Result<QueryBuilder, Error> {
        Ok(QueryBuilder::new(QueryKind::Insert, table).data(model)?)
    }

    pub fn update<T: Serialize>(&self, table: &str, model: T) -> Result<QueryBuilder, Error> {
        Ok(QueryBuilder::new(QueryKind::Update, table).data(model)?)
    }

    pub fn modify<T: Serialize>(&self, table: &str, model: T) -> Result<QueryBuilder, Error> {
        self.update(table, model)
    }

    pub fn remove(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(QueryKind::Remove, table)
    }
}

#[derive(Debug, Clone, Copy)]
enum QueryKind {
    Find,
    List,
    Read,
    Count,
    Insert,
    Update,
    Remove,
}

#[derive(Debug, Clone)]
pub struct QueryBuilder {
    kind: QueryKind,
    table: String,
    fields: Option<String>,
    conditions: Vec<String>,
    sorts: Vec<String>,
    limit: Option<usize>,
    data: Option<Value>,
    missing_error: Option<String>,
}

impl QueryBuilder {
    fn new(kind: QueryKind, table: &str) -> Self {
        Self {
            kind,
            table: table.to_string(),
            fields: None,
            conditions: Vec::new(),
            sorts: Vec::new(),
            limit: None,
            data: None,
            missing_error: None,
        }
    }

    pub fn fields(mut self, fields: &str) -> Self {
        self.fields = Some(fields.to_string());
        self
    }

    pub fn id(mut self, id: impl Serialize) -> Self {
        self.conditions.push(format!(
            "id={}",
            sql_literal_value(&serde_json::to_value(id).unwrap_or(Value::Null))
        ));
        self
    }

    pub fn where_eq(mut self, field: &str, value: impl Serialize) -> Self {
        self.conditions.push(format!(
            "{}={}",
            field,
            sql_literal_value(&serde_json::to_value(value).unwrap_or(Value::Null))
        ));
        self
    }

    pub fn r#where(self, field: &str, value: impl Serialize) -> Self {
        self.where_eq(field, value)
    }

    pub fn query(mut self, condition: &str) -> Self {
        self.conditions.push(condition.to_string());
        self
    }

    pub fn sort(mut self, field: &str, desc: bool) -> Self {
        self.sorts
            .push(format!("{} {}", field, if desc { "DESC" } else { "ASC" }));
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn data<T: Serialize>(mut self, data: T) -> Result<Self, Error> {
        self.data = Some(serde_json::to_value(data).map_err(|err| {
            Error::internal(format!("failed to serialize query builder data: {err}"))
        })?);
        Ok(self)
    }

    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.missing_error = Some(message.into());
        self
    }

    pub fn sql(&self) -> String {
        match self.kind {
            QueryKind::Find | QueryKind::List | QueryKind::Read => self.select_sql(false),
            QueryKind::Count => self.select_sql(true),
            QueryKind::Insert => self.insert_sql(),
            QueryKind::Update => self.update_sql(),
            QueryKind::Remove => self.remove_sql(),
        }
    }

    fn select_sql(&self, count: bool) -> String {
        let fields = if count {
            "COUNT(*)".to_string()
        } else {
            self.fields.clone().unwrap_or_else(|| "*".to_string())
        };
        let mut sql = format!("SELECT {fields} FROM {}", self.table);
        self.push_tail(&mut sql);
        sql
    }

    fn insert_sql(&self) -> String {
        let Some(Value::Object(data)) = &self.data else {
            return format!("INSERT INTO {} DEFAULT VALUES", self.table);
        };
        let columns: Vec<_> = data.keys().cloned().collect();
        let values: Vec<_> = columns
            .iter()
            .map(|key| sql_literal_value(&data[key]))
            .collect();
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table,
            columns.join(", "),
            values.join(", ")
        )
    }

    fn update_sql(&self) -> String {
        let assignments = match &self.data {
            Some(Value::Object(data)) => data
                .iter()
                .map(|(key, value)| format!("{key}={}", sql_literal_value(value)))
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        };
        let mut sql = format!("UPDATE {} SET {}", self.table, assignments);
        self.push_where(&mut sql);
        sql
    }

    fn remove_sql(&self) -> String {
        let mut sql = format!("DELETE FROM {}", self.table);
        self.push_where(&mut sql);
        sql
    }

    fn push_tail(&self, sql: &mut String) {
        self.push_where(sql);
        if !self.sorts.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&self.sorts.join(", "));
        }
        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }
    }

    fn push_where(&self, sql: &mut String) {
        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
        }
    }
}

fn sql_literal_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{}'", value.replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}
