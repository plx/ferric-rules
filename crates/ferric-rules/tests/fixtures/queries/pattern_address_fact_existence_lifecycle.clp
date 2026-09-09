;; A captured fact address becomes non-live after retraction.
;; Level: interaction
;; Covers: queries, fact-existence-lifecycle
(deffacts seed (sample a))
(defrule probe ?f <- (sample a) =>
    (printout t (fact-existp ?f) ":")
    (retract ?f)
    (printout t (fact-existp ?f) crlf))
