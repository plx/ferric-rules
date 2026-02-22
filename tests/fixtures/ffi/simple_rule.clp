(defrule hello-world
  (initial-fact)
  =>
  (printout t "Hello from FFI" crlf))
