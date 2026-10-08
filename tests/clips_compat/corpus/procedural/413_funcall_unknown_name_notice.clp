(deffacts start (phase go))
(defrule kick (phase go) => (focus B))
(defmodule A)
(deffunction hidden () 7)
(defmodule B)
(deffunction shown () 8)
(defrule B::go
   =>
   (printout t "missing=" (funcall nosuch) crlf)
   (printout t "hidden=" (funcall hidden) crlf)
   (printout t "shown=" (funcall shown) crlf)
   (printout t "qualified=" (funcall MAIN::nosuch) crlf)
   (printout t "qhidden=" (funcall A::hidden) crlf)
   (printout t "qshown=" (funcall B::shown) crlf))
