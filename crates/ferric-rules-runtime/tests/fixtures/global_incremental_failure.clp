; A failed named initializer must not declare its name or later names.
(defmodule A (export ?ALL))
(defglobal ?*first* = 1 ?*second* = (+ ?*first* 1) ?*bad* = (/ 1 0) ?*last* = 99)
(defglobal ?*following* = 4)
(defmodule B (export ?ALL))
(defglobal ?*last* = 7)
(defmodule USE (import A ?ALL) (import B ?ALL))
(defglobal ?*answer* = (+ ?*first* ?*second* ?*following* ?*last*))
(defrule report => (printout t ?*answer* crlf))
