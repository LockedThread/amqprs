FROM rabbitmq:3.13-management
RUN rabbitmq-plugins enable --offline rabbitmq_management rabbitmq_management_agent rabbitmq_auth_mechanism_ssl
