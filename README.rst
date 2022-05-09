🍦Rustserve
============

Rustserve is a runtime agnostic async HTTP web framework written in Rust.  


📖How it works
--------------

The main component in a Rustserve webserver is a `Controller`.  Controllers have
methods that map to the `HTTP` methods, `GET`, `POST`, `PUT`, etc.  All of the
`Controller` methods have defaults that return 404, you don't have to implement
any of them.  The `Controller` trait, along with a `Route` allow you to define
custom behavior for a route at a specific `HTTP` method.  

Throughout this README we are going to build up an example server.  The full
implementation can be found `here`. Lets implement a `Controller`.  

.. code-block:: rust

   struct MyController {

   }

   impl Controller for MyController {

   }

   impl HttpError for MyController {

   }

That's it!  Lets go over some details about whats going on here.  

.. code-block:: rust

   impl Controller for MyController {

   }

This is the implementation of `Controller` for the `MyController` struct.  All
of the methods in the `Controller` trait have defaults that return 404.  Lets
implement the `post` method for `MyController` so that it returns a 200 instead

.. code-block:: rust

   impl Controller for MyController {
      fn post<'a>(
          self: Arc<Self>,
          _req: Request<&'a [u8]>,
          _params: HashMap<String, String>,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          Box::pin(async move {
            Ok(serialize(Response::builder().status(200).body(())?)?)
          })
      }
   }

Ok! There is a lot going on here, lets break it down

.. code-block:: rust

   impl Controller for MyController {
      // We are implementing the `post` method, meaning POST requests to this
      // controller will use our `post` implementation
      fn post<'a>(
          // Self is an Arc<Self>, meaning that you can only use controller
          // methods when your controller is an Arc<dyn Controller>.  
          self: Arc<Self>,
          // The request with its body as a series of bytes
          _req: Request<&'a [u8]>,
          // The params parsed out of the uri such as /:version/
          _params: HashMap<String, String>,
      // The return type is a Future that outputs a Result<Response<Vec<u8>>>
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          // Create a Future
          Box::pin(async move {
            // Return a Result<Response<Vec<u8>>> by passing a 200 Response to the serialize function
            Ok(serialize(Response::builder().status(200).body(())?)?)
          })
      }
    }

Cool! So now POST requests to this controller return a 200.  Lets implement an
`HttpError` method so we can understand more about how our errors are being
handled in our post implementation. 

.. code-block:: rust

   impl HttpError for MyController {
      fn internal_server_error(
          self: Arc<Self>,
          e: anyhow::Error,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
          Box::pin(async move {
              Ok(serialize(
                  Response::builder().status(500).body(format!("{e}"))?,
              )?)
          })
      }
   }

Whew! Again there is a lot going on, lets break it down

.. code-block:: rust

   impl HttpError for MyController {
      // We are implementing the `internal_server_error` method, this means when
      // internal_server_error is called on this controller, it will use our
      // implementation.
      fn internal_server_error(
          // Self is an Arc<Self>, meaning that you can only use controller
          // methods when your controller is an Arc<dyn HttpError>.   
          self: Arc<Self>,
          // The error that ocurred is passed as a parameter.  
          e: anyhow::Error,
      // The return type is a Future that outputs a Result<Response<Vec<u8>>>
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
          // Create a future
          Box::pin(async move {
              // Return a Result<Response<Vec<u8>>> by passing a 500 response to
              // the serialize function.  We are also setting the body of the
              // response to be the error we received as an argument.  
              Ok(serialize(
                  Response::builder().status(500).body(format!("{e}"))?,
              )?)
          })
      }
   }


