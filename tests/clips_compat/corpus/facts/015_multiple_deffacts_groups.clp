; Reset loads every deffacts group into the same working memory.
;; Level: interaction
;; Covers: facts, multiple-deffacts-groups
; Protocol: load, reset, run to quiescence.
(deffacts first-group (left a))
(deffacts second-group (right b))
(defrule observe
  (left ?left) (right ?right)
  => (printout t ?left " " ?right crlf))
