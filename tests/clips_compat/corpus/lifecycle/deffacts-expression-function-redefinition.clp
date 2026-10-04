;; Reset uses a function body replaced after the seed definition was loaded.
;; Level: interaction
;; Covers: assertion-expression, deffacts, reset, deffunction
;; Resets: 2
(deffunction current () old)
(deffacts seed (value (current)))
(deffunction current () new)
(defrule show (value ?x) => (printout t ?x crlf))
