(defmodule A)
(defmodule MAIN)
(defrule run =>
  (printout t "result:["
    (focus (if TRUE then (printout t "unexpected-left") A) (if TRUE then (printout t M) MISSING))
    "] stack:" (get-focus-stack) crlf))
