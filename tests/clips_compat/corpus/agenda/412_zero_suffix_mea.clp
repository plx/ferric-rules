(deffacts seed (u 3))
(defrule base (u ?x) => (printout t BASE crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule longer (u ?x) (not (missing)) => (printout t ZERO-LONG crlf))"))
