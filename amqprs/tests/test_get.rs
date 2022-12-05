use amqprs::{
    channel::{
        BasicAckArguments, BasicGetArguments, BasicPublishArguments, QueueBindArguments,
        QueueDeclareArguments,
    },
    connection::{Connection, OpenConnectionArguments},
    BasicProperties,
};
use tracing::{info, Level};
mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get() {
    let _guard = common::setup_logging(Level::INFO);

    // open a connection to RabbitMQ server
    let args = OpenConnectionArguments::new("localhost:5672", "user", "bitnami");

    let connection = Connection::open(&args).await.unwrap();

    // open a channel on the connection
    let channel = connection.open_channel(None).await.unwrap();

    let exchange_name = "amq.topic";
    // declare a queue
    let (queue_name, ..) = channel
        .queue_declare(QueueDeclareArguments::default())
        .await
        .unwrap()
        .unwrap();

    // bind the queue to exchange
    let routing_key = "get.test"; // key should also be used by publish
    channel
        .queue_bind(QueueBindArguments::new(
            &queue_name,
            exchange_name,
            routing_key,
        ))
        .await
        .unwrap();

    // get empty
    let get_args = BasicGetArguments::new(&queue_name);

    // contents to publish
    let content = String::from(
        r#"
            {
                "data": "some data to publish for test"
            }
        "#,
    )
    .into_bytes();

    // create arguments for basic_publish
    let args = BasicPublishArguments::new(&exchange_name, routing_key);

    let num_loop = 3;
    for _ in 0..num_loop {
        channel
            .basic_publish(BasicProperties::default(), content.clone(), args.clone())
            .await
            .unwrap();
    }

    for i in 0..num_loop {
        // get single message
        let delivery_tag = match channel.basic_get(get_args.clone()).await.unwrap() {
            Some((get_ok, basic_props, content)) => {
                #[cfg(feature="tracing")]
                info!(
                    "Get results: 
                    {}
                    {}
                    Content: {}",
                    get_ok,
                    basic_props,
                    std::str::from_utf8(&content).unwrap()
                );
                // message count should decrement accordingly
                assert_eq!(num_loop - 1 - i, get_ok.message_count());
                get_ok.delivery_tag()
            }
            None => panic!("expect get a message"),
        };
        // ack to received message
        channel
            .basic_ack(BasicAckArguments {
                delivery_tag,
                multiple: false,
            })
            .await
            .unwrap();
    }

    // explicitly close
    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_empty() {
    let _guard = common::setup_logging(Level::INFO);

    // open a connection to RabbitMQ server
    let args = OpenConnectionArguments::new("localhost:5672", "user", "bitnami");

    let connection = Connection::open(&args).await.unwrap();

    // open a channel on the connection
    let channel = connection.open_channel(None).await.unwrap();

    let exchange_name = "amq.topic";
    // declare a queue
    let (queue_name, ..) = channel
        .queue_declare(QueueDeclareArguments::default())
        .await
        .unwrap()
        .unwrap();

    // bind the queue to exchange
    channel
        .queue_bind(QueueBindArguments::new(
            &queue_name,
            exchange_name,
            "__no_one_use_this_key__", // this make sure we receive a empty response
        ))
        .await
        .unwrap();

    // get empty
    let get_message = channel
        .basic_get(BasicGetArguments::new(&queue_name))
        .await
        .unwrap();
    if let Some(_) = get_message {
        panic!("expect ReturnEmpty message");
    }
}
