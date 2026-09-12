; A slot modification cancels stale matches and enables new matches.
;; Level: interaction
;; Covers: agenda, modify-cancels-old-match
; Protocol: load, reset, run to quiescence.
(deftemplate item (slot state))
(deffacts input (item (state old)))
(defrule update
  (declare (salience 10)) ?f <- (item (state old))
  => (modify ?f (state new)))
(defrule stale (item (state old)) => (printout t "stale" crlf))
(defrule observe (item (state new)) => (printout t "new" crlf))
