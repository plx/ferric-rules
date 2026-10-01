;; Issue #323: a call with no applicable method does not run the body.
(defgeneric guarded)
(defmethod guarded ()
  (printout t "body" crlf))
(defrule probe => (guarded 1) (printout t "after" crlf))
