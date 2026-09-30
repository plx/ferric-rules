;; An instance name is its own type in facts and patterns: an INSTANCE-NAME
;; literal matches only instance names, in slots, multislots and ordered facts.
;; Level: boundary
;; Covers: instance-name, deftemplate, multislot, ordered-facts, eq
(deftemplate obj (slot name (type INSTANCE-NAME)) (multislot refs))
(deffacts seed
  (obj (name [a]) (refs [b] b "b"))
  (obj (refs))
  (link [a] a))
(defrule by-name (obj (name [a]) (refs $?r)) => (printout t "by-name " ?r crlf))
(defrule ref-instance (obj (refs $? [b] $?)) => (printout t "ref [b]" crlf))
(defrule ref-symbol (obj (refs $? b $?)) => (printout t "ref b" crlf))
(defrule ref-string (obj (refs $? "b" $?)) => (printout t "ref string" crlf))
(defrule default-name (obj (name ?n) (refs)) => (printout t "default " ?n " " (instance-namep ?n) crlf))
(defrule ordered (link ?x ?y)
  => (printout t ?x " " (instance-namep ?x) " " ?y " " (instance-namep ?y) " " (eq ?x ?y) crlf))
(defrule symbol-first (link a $?) => (printout t "symbol a" crlf))
(defrule instance-first (link [a] $?) => (printout t "instance [a]" crlf))
