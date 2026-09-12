;; bind updates an already declared global.
;; Level: basic
;; Covers: modules, global-bind
(defglobal ?*count* = 2)
(defrule probe =>
    (bind ?*count* (+ ?*count* 5))
    (printout t ?*count* crlf))
