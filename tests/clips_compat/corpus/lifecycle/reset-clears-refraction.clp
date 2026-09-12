;; Level: interaction
;; Covers: reset, refraction
;; Resets: 2
(deffacts seed (seed))
(defrule probe (seed) => (printout t "fire" crlf))
