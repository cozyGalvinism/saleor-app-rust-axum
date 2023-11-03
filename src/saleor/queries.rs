use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "schema.json",
    query_path = "queries/my_id.graphql"
)]
pub struct MyId;

impl MyId {
    pub fn variables() -> my_id::Variables {
        my_id::Variables {}
    }
}
