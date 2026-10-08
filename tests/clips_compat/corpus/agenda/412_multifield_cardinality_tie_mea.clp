(deffacts seed (v 1 2 3))
(defrule whole (v $?a) => (printout t WHOLE crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule headed (v ? $?b) => (printout t HEADED crlf))"))
