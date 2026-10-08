(deffacts seed (u 3))
(defrule wildcard (u ?) => (printout t WILDCARD crlf))
(defrule constant (u 3) => (printout t CONSTANT crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule returned (u =(+ 1 2)) => (printout t RETURNED crlf))")
  (build "(defrule negated (u ~4) => (printout t NEGATED crlf))"))
