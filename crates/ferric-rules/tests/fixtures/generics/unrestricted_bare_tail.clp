;; Issue #323: unrestricted defmethod parameter compatibility.
(defgeneric tail)

;; BEGIN METHODS
(defmethod tail (?head $?rest)
  (if (= (length$ ?rest) 0)
    then (str-cat ?head ":0")
    else (str-cat ?head ":" (length$ ?rest)
      ":" (nth$ 1 ?rest) ":" (nth$ 2 ?rest) ":" (nth$ 3 ?rest)
      ":" (stringp (nth$ 3 ?rest)))))
;; END METHODS

(defrule probe
  =>
  (printout t (tail first) crlf (tail first a 2 "three") crlf))
