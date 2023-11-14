#[cynic::schema("saleor")]
mod schema {}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "Query")]
pub struct MyId {
    pub me: Option<MeId>,
}

#[derive(cynic::QueryFragment, Debug)]
#[cynic(graphql_type = "User")]
pub struct MeId {
    pub id: cynic::Id,
}
