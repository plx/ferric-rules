(deffacts seed (u 3))
(defrule plain (u ?x&:(> ?x 0)) => (printout t PLAIN crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule nested (u ?x&:(> (+ ?x 1) 0)) => (printout t NESTED crlf))")
  (build "(defrule logical (u ?x&:(and (> ?x 0) (< ?x 10))) => (printout t LOGICAL crlf))"))
