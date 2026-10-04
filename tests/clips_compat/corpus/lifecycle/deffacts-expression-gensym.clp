;; Seed expressions execute on every reset, without running during source loading.
;; Level: interaction
;; Covers: assertion-expression, deffacts, reset, gensym*
;; Resets: 2
(deffacts seed (value (gensym*)))
(defrule show (value ?x) => (printout t ?x crlf))
