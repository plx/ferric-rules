; Refraction: a rule fires at most once per token
; The single (item a) fact should trigger the rule exactly once.
(deffacts startup (item a))

(defrule process
    (item ?x)
    =>
    (printout t "processed " ?x crlf))
