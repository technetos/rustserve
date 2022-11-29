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

That's it!  Lets go over some details about whats going on here.  

.. code-block:: rust

   impl Controller for MyController {

   }

This is the implementation of `Controller` for the `MyController` struct.  All
of the methods in the `Controller` trait have defaults that return 404.  Lets
implement the `get` method for `MyController` so that it returns a 200 instead

.. code-block:: rust

   impl Controller for MyController {
      fn get<'a>(
          self: Arc<Self>,
          _req: Request<&'a [u8]>,
          _params: HashMap<String, String>,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          Box::pin(async move {
            Ok(Response::builder().status(200).body(vec![])?)
          })
      }
   }

Ok! There is a lot going on here, lets break it down

.. code-block:: rust

   impl Controller for MyController {
      // We are implementing the `get` method, meaning GET requests to this
      // controller will use our `get` implementation
      fn get<'a>(
          // Self is an Arc<Self>, meaning that you can only use controller
          // methods when your controller is an Arc<dyn Controller>.  
          self: Arc<Self>,
          // The request with its body as a series of bytes
          _req: Request<&'a [u8]>,
          // The params parsed out of the uri such as /:version/
          _params: HashMap<String, String>,
      // The return type is a Future that outputs a Result<Response<Vec<u8>>>
      ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
          // Create a Future
          Box::pin(async move {
            // Return a Result<Response<Vec<u8>>>
            Ok(Response::builder().status(200).body(vec![])?)
          })
      }
    }


Cool! So now GET requests to this controller return a 200 and an empty body. At
this point we have covered the basics of the `Controller` trait.  It provides
the standard HTTP methods to `MyController` and when we implement any of the
`Controller` methods, those methods get used when a request comes in.

Error
-----

Lets implement an `Error<'a, T>` method so we can understand more about how our
errors are being handled in our `get` implementation.

.. code-block:: rust

   #[derive(serde::Deserialize)]
   struct MyError;

   impl<'a> Error<'a, MyError, 500> for MyController {}

Whew! Again there is a lot going on, lets break it down

.. code-block:: rust

   // Derive serialize for the error message
   #[derive(serde::Serialize)]
   // The actual error message
   struct MyError;

   // Implementation of the Error trait parameterized with our message and the
   // HTTP status code for the error.
   impl<'a> Error<'a, MyError, 500> for MyController {}

So whats going on here? We implemented the `get` method, that seems to make
sense, but we are never using `MyError`, why is it necessary? 

That's a great question.  Various kinds of errors can occur in your web server,
sometimes something is broken or wrong resulting in a 500 or a request is
malformed and results in a 400, all of these errors can be represented by the
`Error<'a, T>` trait.  The idea is that when something goes wrong in your
controller, your controller defines what happens on a per error basis.  

With that in mind, lets re-implement the `get` method implementation to return
an error.

.. code-block:: rust


   impl Controller for MyController {
      fn get<'a>(
          self: Arc<Self>,
          _req: Request<&'a [u8]>,
          _params: HashMap<String, String>,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          Box::pin(async move {
              self.error(MyError {}).await
          })
      }
    }

Wow! Thats actually really simple.  When we implement the `Error` trait for
`MyController` the controller gains a method called `self.error`. Lets dive into
the `Error` trait a bit more.


.. code-block:: rust

   impl<'a> Error<'a, MyError, 500> for MyController {}

   //                    ^      ^            ^
   //                    |      |            |
   //                    |      |            |

Lets break this down.  First we can see that the `Error` trait takes in a few
generic arguments, `MyError` and `500`.  The first generic parameter is the
error message, it can be anything that implements `serde::Serialize`, the second
argument is the HTTP status code that accompanies the returned error.  This
means that we can use the same error message with different error codes.  Next
we can see that we are implementing the `Error` trait for `MyController`, this
provides `MyController` a `self.error` method that expects an instance of
`MyError`.  As we can see in the example above, `self.error` is an async
function and must be `.await'ed`.  The `Error` trait can be implemented multiple
times for the same controller.  This allows controllers to use `self.error` with
any error messages supported by the controller. The returned type from calling
`self.error` is a future that yields `anyhow::Result<Response<Vec<u8>>` or in
other terms, a future that yields an HTTP response with the serialized message
as the response body.

Reply
-----

Cool! Now we know how to return errors, but what if we want to return a 200 ok
with a response body?

Thats another great question! Lets take another look at our example where we
returned a 200 ok with an empty body.

.. code-block:: rust

   impl Controller for MyController {
      fn get<'a>(
          self: Arc<Self>,
          _req: Request<&'a [u8]>,
          _params: HashMap<String, String>,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          Box::pin(async move {
            Ok(Response::builder().status(200).body(vec![])?)
          })
      }
   }

Here we are manually crafting a response with a 200 status code and empty body,
Thats fine but theres a better way.

.. code-block:: rust

   #[derive(serde::Serialize)]
   struct MyMessage {}

   impl Reply<'a, MyMessage> for MyController {}

   impl Controller for MyController {
      fn get<'a>(
          self: Arc<Self>,
          _req: Request<&'a [u8]>,
          _params: HashMap<String, String>,
      ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
          Box::pin(async move {
              self.reply(MyMessage).await
          })
      }
    }

Woah woah slow down, whats this `Reply` trait?

Thats another great question! Lets dive into it.
