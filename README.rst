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
          methods when your controller is an Arc<dyn Controller>.  
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
   
