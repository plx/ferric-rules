; RH-CORE-026: RHS assert uses named template slots and preserves default typed values.
(deftemplate record (slot name (type SYMBOL)) (slot count (type INTEGER) (default 7)) (slot label (type STRING) (default "ready")))
(deffacts seed (go))
(defrule create-record ?g <- (go) => (retract ?g) (assert (record (name alice))))
(defrule read-record (record (name ?n) (count ?c) (label ?s)) => (printout t ?n " " ?c " " ?s crlf) (assert (result ?n ?c ?s)))
