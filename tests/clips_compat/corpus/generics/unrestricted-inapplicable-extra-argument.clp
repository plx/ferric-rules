;; Issue #323: a call with no applicable method does not run the body.
(defgeneric guarded)
(defmethod guarded (?x)
  (printout t "body" crlf))
(defrule probe => (guarded 1 2) (printout t "after" crlf))
