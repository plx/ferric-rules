(deffacts seed (u 3))
(defrule specific (u ?x&:(> ?x 1)&:(< ?x 100)) (test (> ?x 2))
  => (printout t SPECIFIC crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule general (u ?) => (printout t GENERAL crlf))"))
