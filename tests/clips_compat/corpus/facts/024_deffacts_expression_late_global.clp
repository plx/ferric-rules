;; Seed expressions resolve globals that are defined after the seed source is loaded.
;; Level: interaction
;; Covers: assertion-expression, deffacts, defglobal, variable-scope
(deffacts seed (value ?*late* (+ ?*late* 1)))
(defglobal ?*late* = 7)
(defrule show (value ?direct ?computed) => (printout t ?direct ":" ?computed crlf))
