(defmodule MAIN (export ?ALL))
(deffacts MAIN::start (phase go))
(defrule MAIN::kick (phase go) => (focus A))
(defmodule A (import MAIN ?ALL))
(deffunction f () 7)
(defrule A::go
   ?phase <- (phase go)
   =>
   (retract ?phase)
   (printout t "plain=" (f) crlf)
   (printout t "ev=" (eval "(f)") crlf)
   (printout t "r=" (eval "(progn (reset) (f))") crlf)
   (halt))
